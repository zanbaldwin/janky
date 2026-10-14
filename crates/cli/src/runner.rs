//! The job runner of `janky generate`: it calls the `prepare` function of each generator
//! instance once, then the `generate` function of each job, and stores the results in the media
//! cache or the data cache.
//!
//! Each job is a Lua coroutine. [`run`] calls `generate` as an async Lua function, and a Tokio
//! runtime with one thread drives all jobs on the one Lua state. A host function that waits
//! (`janky.http.request`, `janky.http.download`, `janky.wait` and the iterator of
//! `janky.fs.json_items`) gives control back to the runner, which then resumes other jobs. The
//! plugin code reads as if it blocks.
//!
//! The limits of [`crate::lua`] apply to each resume, not to the whole job. Before each resume
//! the runner calls [`Sandbox::begin`], and after it [`Sandbox::end`]. So the instruction count
//! and the watchdog clock start again, and time spent waiting does not count. Over all its
//! resumes, a job may run at most [`JOB_INSTRUCTION_LIMIT`] instructions and
//! [`JOB_TIME_LIMIT`], waits included. A `prepare` call has the same limits as one job.
//!
//! The host functions that do I/O exist only during [`run`]. It adds `janky.http.request`,
//! `janky.http.download`, `janky.secret`, `janky.wait`, `janky.cache` and `janky.fs` to the
//! `janky` table of each generator instance, and removes them at the end. They also refuse to
//! work when no job or `prepare` call is being resumed, so a filter that kept a reference to one
//! cannot use it. Filters, functions and `plan` therefore stay offline.
//!
//! The [`Http`] trait sends the requests. [`crate::http::Client`] is the real client.
//! [`FakeHttp`] gives recorded responses to tests.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::future::{Either, select};
use futures_util::stream::{FuturesUnordered, StreamExt};
use mlua::{MultiValue, Table, Value};

use crate::cache::{self, Meta};
use crate::diagnostic::Diagnostic;
use crate::generate::{Job, Kind, valid_ext};
use crate::lua::Sandbox;
use crate::plugin::{Plugin, Plugins, lua_error, set_variables};

/// The most requests, downloads included, that one job or one `prepare` call may send.
pub const REQUEST_LIMIT: usize = 5;
/// The longest wait of one `janky.wait` call. A longer wait is cut to this.
pub const WAIT_LIMIT: Duration = Duration::from_secs(60);
/// The longest time one job may take, waits included.
pub const JOB_TIME_LIMIT: Duration = Duration::from_secs(600);
/// The most Lua instructions of one job, over all its resumes.
pub const JOB_INSTRUCTION_LIMIT: u64 = 200_000_000;
const FORBIDDEN_HEADERS: [&str; 9] = [
    "host",
    "accept-encoding",
    "transfer-encoding",
    "content-length",
    "connection",
    "te",
    "upgrade",
    "keep-alive",
    "trailer",
];
const METHODS: [&str; 6] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"];

/// A request header value as the plugin gave it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum HeaderText {
    /// Plain text, without line breaks.
    Text(String),
    /// A secret handle. The [`Http`] implementation puts `prefix` and then the secret value in
    /// the header when it sends the request. The plugin never sees the value.
    Secret { name: String, prefix: String },
}

/// A request from `janky.http.request`, after the checks that need no settings.
///
/// The runner has checked the method and the headers. The [`Http`] implementation checks the
/// URL against the instance's allowed hosts.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// One of `GET`, `POST`, `PUT`, `PATCH`, `DELETE` and `HEAD`, in upper case.
    pub method: String,
    /// The URL as the plugin gave it. The runner does not check it.
    pub url: String,
    /// At most 64 headers, sorted. A plugin cannot set `Host`, `Content-Length`, `Proxy-*` and
    /// the other headers that control the connection.
    pub headers: Vec<(String, HeaderText)>,
    /// The request body. It is empty when the plugin gave no `body`.
    pub body: Vec<u8>,
}

/// The opaque handle that `janky.secret(name, prefix)` gives to a plugin.
///
/// It holds the secret's name, not its value. It can only be a header value: `tostring` gives
/// `<secret:name>`, and `..` is an error.
#[derive(Debug, Clone)]
pub struct Secret {
    /// The local name, from the plugin's `secrets` list.
    pub name: String,
    /// The text before the value, for example `Bearer `. At most 64 bytes, with no control
    /// characters.
    pub prefix: String,
}

impl mlua::UserData for Secret {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(mlua::MetaMethod::ToString, |_, secret, ()| Ok(format!("<secret:{}>", secret.name)));
        methods.add_meta_function(mlua::MetaMethod::Concat, |_, _: (Value, Value)| {
            Err::<(), _>(mlua::Error::runtime("a secret cannot be joined with `..`: give the text before it as the prefix, janky.secret(name, prefix)"))
        });
    }
}

/// The response that an [`Http`] implementation gives back.
///
/// The plugin gets the headers with lower-case names. The runner joins the values of repeated
/// headers with `, `.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    /// The HTTP status code of the last attempt.
    pub status: u16,
    /// The response headers. A name can occur more than once.
    pub headers: Vec<(String, String)>,
    /// The complete body. [`crate::http::Client`] refuses a body larger than
    /// [`crate::http::BODY_LIMIT`].
    pub body: Vec<u8>,
}

/// A request in progress. An `Err` is a message that the job gets as a Lua error.
pub type Sending = Pin<Box<dyn Future<Output = Result<Response, String>> + Send>>;

/// The result of a download: the status, the headers as for a [`Response`], and the number of
/// bytes written to the file. A status other than 2xx writes nothing and has the size 0.
#[derive(Debug, Clone, PartialEq)]
pub struct Downloaded {
    /// The HTTP status code of the last attempt.
    pub status: u16,
    /// The response headers, as for [`Response::headers`].
    pub headers: Vec<(String, String)>,
    /// The number of bytes written to the file.
    pub size: u64,
}

/// A download in progress. An `Err` is a message that the job gets as a Lua error.
pub type Downloading = Pin<Box<dyn Future<Output = Result<Downloaded, String>> + Send>>;

/// Sends the requests of all jobs.
pub trait Http: Send + Sync {
    /// Sends `request` for the plugin instance `instance`. The instance name selects its
    /// allowed hosts, its secrets and its limits. A wait before a retry uses `pause`.
    fn send(&self, instance: &str, request: Request, pause: Pause) -> Sending;

    /// Sends `request` like [`send`](Self::send), and writes a 2xx body to `file` instead of
    /// giving it back. `file` is in the instance's file area (see [`cache::plugin_file`]).
    fn download(&self, instance: &str, request: Request, file: PathBuf, pause: Pause) -> Downloading;
}

/// The number of jobs of each instance that count against its limit in [`run`].
#[derive(Clone, Default)]
struct Places {
    counts: Arc<Mutex<HashMap<String, usize>>>,
    freed: Arc<tokio::sync::Notify>,
}

impl Places {
    fn count(&self, instance: &str) -> usize {
        self.counts.lock().expect("places").get(instance).copied().unwrap_or(0)
    }

    fn add(&self, instance: &str) {
        *self.counts.lock().expect("places").entry(instance.to_string()).or_default() += 1;
    }

    fn remove(&self, instance: &str) {
        *self.counts.lock().expect("places").entry(instance.to_string()).or_default() -= 1;
    }
}

/// The handle that [`Http::send`] uses to wait before a retry.
pub struct Pause {
    places: Places,
    instance: String,
}

impl Pause {
    /// Waits for `duration`. During the wait the job does not count against the limit of its
    /// instance, so the runner can start another job of that instance.
    pub async fn sleep(&self, duration: Duration) {
        struct Back<'a>(&'a Pause);
        impl Drop for Back<'_> {
            fn drop(&mut self) {
                self.0.places.add(&self.0.instance);
            }
        }
        self.places.remove(&self.instance);
        self.places.freed.notify_one();
        let _back = Back(self);
        tokio::time::sleep(duration).await;
    }
}

/// An [`Http`] for tests, which records each request and gives back recorded responses.
#[derive(Default)]
pub struct FakeHttp {
    /// The responses, in the order of the requests. When the list is empty, each request
    /// gets the error "no recorded response".
    pub responses: Mutex<VecDeque<Result<Response, String>>>,
    /// Each request that a job sent, with its instance name.
    pub requests: Mutex<Vec<(String, Request)>>,
    /// The time before each response arrives.
    pub delay: Duration,
}

impl FakeHttp {
    /// Makes a fake that gives back `responses` with no delay.
    pub fn new(responses: Vec<Result<Response, String>>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            ..Self::default()
        }
    }
}

impl Http for FakeHttp {
    fn send(&self, instance: &str, request: Request, _pause: Pause) -> Sending {
        self.requests.lock().expect("fake requests").push((instance.to_string(), request));
        let response =
            self.responses.lock().expect("fake responses").pop_front().unwrap_or(Err("no recorded response".into()));
        let delay = self.delay;
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            response
        })
    }

    fn download(&self, instance: &str, request: Request, file: PathBuf, pause: Pause) -> Downloading {
        let sending = self.send(instance, request, pause);
        Box::pin(async move {
            let response = sending.await?;
            let size = if (200..300).contains(&response.status) {
                std::fs::write(&file, &response.body).map_err(|error| error.to_string())?;
                response.body.len() as u64
            } else {
                0
            };
            Ok(Downloaded {
                status: response.status,
                headers: response.headers,
                size,
            })
        })
    }
}

/// What happened to one job.
#[derive(Debug)]
pub enum Outcome {
    /// The result is in the cache as `file`. `cost` is what `generate` reported, if anything.
    ///
    /// `file` is the file name in the media cache (`<hash>.<ext>`) or in the data cache
    /// (`<hash>.json`).
    Stored { file: String, cost: Option<f64> },
    /// `generate` returned `nil` and this message ("no reason given" without one).
    Refused(String),
    /// The job was refused or failed, and its last response was a 401 or 403 to a request
    /// that used secrets. `secrets` names them, and `message` is the reason or the error
    /// message. A wrong key or an empty quota gives this outcome.
    Unauthorised { message: String, secrets: Vec<String> },
    /// The job did not give a usable result: a Lua error, a limit, a wrong result table, or a
    /// cache write that failed.
    Failed(Diagnostic),
    /// The job did not start, because the run stopped.
    Skipped,
}

#[derive(Clone, Default)]
struct JobContext {
    requests: Arc<AtomicUsize>,
    refused: Arc<Mutex<Vec<String>>>,
    /// The call is `prepare`, so `janky.cache` writes go into one transaction.
    prepare: bool,
}

type Current = Arc<Mutex<Option<JobContext>>>;

struct Guarded<F> {
    sandbox: Arc<Sandbox>,
    label: String,
    context: JobContext,
    current: Current,
    used: u64,
    inner: Option<Pin<Box<F>>>,
}

impl<F: Future<Output = mlua::Result<MultiValue>>> Future for Guarded<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = self.get_mut();
        let Some(inner) = this.inner.as_mut() else {
            return Poll::Ready(Err(mlua::Error::runtime("the job has already finished")));
        };
        this.sandbox.begin(&this.label);
        *this.current.lock().expect("current job") = Some(this.context.clone());
        let mut result = inner.as_mut().poll(cx);
        this.used += this.sandbox.used();
        if result.is_pending() && this.used > JOB_INSTRUCTION_LIMIT {
            result = Poll::Ready(Err(mlua::Error::runtime(format!(
                "the job ran more than {JOB_INSTRUCTION_LIMIT} instructions in total"
            ))));
        }
        if result.is_ready() {
            this.inner = None;
        }
        *this.current.lock().expect("current job") = None;
        this.sandbox.end();
        result
    }
}

impl<F> Drop for Guarded<F> {
    fn drop(&mut self) {
        if self.inner.is_some() {
            self.sandbox.begin(&self.label);
            self.inner = None;
            self.sandbox.end();
        }
    }
}

/// The most jobs that each plugin instance may run at the same time, by instance name.
///
/// It is `max_concurrency` of `[plugins.<name>.http]`, at least 1. An instance without that
/// table gets 1.
pub fn limits(project: &crate::project::Project) -> HashMap<String, usize> {
    project
        .deck
        .plugins
        .iter()
        .map(|(name, instance)| {
            (name.clone(), instance.http.as_ref().map_or(1, |http| http.max_concurrency.max(1) as usize))
        })
        .collect()
}

/// Runs `jobs` and gives back each job with its [`Outcome`].
///
/// Jobs start in the given order, with at most the number from `limits` in flight for each
/// instance (1 for an instance that is not in `limits`). A job that waits before a retry
/// ([`Pause::sleep`]) is not in flight. Each result goes into the cache under
/// `root` when its job ends, before the next job starts. Then the runner calls `done`. When
/// `done` gives `false`, no more jobs start: the jobs in flight finish, and the others are
/// [`Outcome::Skipped`]. The list holds the finished jobs in the order they ended, then the
/// skipped jobs.
///
/// The runner calls `starting` with the instance name when it starts the `prepare` function
/// of that instance. The jobs of an instance with `prepare` wait until that call ends. When it
/// fails, each of these jobs gets its failure, as [`Outcome::Failed`] or
/// [`Outcome::Unauthorised`]. The `prepare` call counts against the limit of its instance.
///
/// A job fails when its instance has no `generate` function. When the runner cannot add the
/// host functions or start Tokio, every job fails with the same diagnostic.
pub fn run(
    plugins: &Plugins,
    root: &Path,
    jobs: Vec<Job>,
    limits: &HashMap<String, usize>,
    http: Arc<dyn Http>,
    mut starting: impl FnMut(&str),
    mut done: impl FnMut(&Job, &Outcome) -> bool,
) -> Vec<(Job, Outcome)> {
    let generators: HashMap<&str, &Plugin> = plugins
        .plugins
        .iter()
        .filter(|plugin| plugin.instance && plugin.generate.is_some())
        .map(|plugin| (plugin.name.as_str(), plugin))
        .collect();
    let current: Current = Arc::default();
    let places = Places::default();
    let caches: Caches = Arc::default();
    let streams: Streams = Arc::default();
    let shared = Shared {
        http: http.clone(),
        current: current.clone(),
        places: places.clone(),
        caches: caches.clone(),
        streams: streams.clone(),
    };
    let mut results = Vec::new();
    for plugin in generators.values() {
        if let Err(error) = install(&plugins.sandbox, plugin, root, shared.clone()) {
            for plugin in generators.values() {
                let _ = uninstall(plugin);
            }
            results.extend(jobs.into_iter().map(|job| {
                let failure = Diagnostic::error("generate::host", format!("cannot set up `{}`: {error}", plugin.name));
                (job, Outcome::Failed(failure))
            }));
            return results;
        }
    }
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            let failure = Diagnostic::error("generate::runtime", format!("cannot start the job runner: {error}"));
            return jobs.into_iter().map(|job| (job, Outcome::Failed(failure.clone()))).collect();
        },
    };
    runtime.block_on(async {
        let mut queue: VecDeque<Job> = jobs.into();
        let mut running: FuturesUnordered<Pin<Box<dyn Future<Output = Finished>>>> = FuturesUnordered::new();
        let mut preparing: HashMap<String, Preparing> = HashMap::new();
        let mut stopped = false;
        loop {
            let mut early = Vec::new();
            if !stopped {
                let mut waiting = VecDeque::new();
                while let Some(job) = queue.pop_front() {
                    let prepare = generators.get(job.instance.as_str()).filter(|plugin| plugin.prepare.is_some());
                    if let Some(plugin) = prepare {
                        match preparing.get(&job.instance) {
                            None => {
                                preparing.insert(job.instance.clone(), Preparing::Running);
                                places.add(&job.instance);
                                starting(&job.instance);
                                let instance = job.instance.clone();
                                let prepared = start_prepare(plugins, plugin, current.clone());
                                running.push(Box::pin(async move { Finished::Prepared(instance, prepared.await) }));
                                waiting.push_back(job);
                                continue;
                            },
                            Some(Preparing::Running) => {
                                waiting.push_back(job);
                                continue;
                            },
                            Some(Preparing::Done(Err(failure))) => {
                                early.push((job, failure.outcome()));
                                continue;
                            },
                            Some(Preparing::Done(Ok(()))) => {},
                        }
                    }
                    let limit = limits.get(&job.instance).copied().unwrap_or(1).max(1);
                    if places.count(&job.instance) < limit {
                        places.add(&job.instance);
                        let started = start(plugins, &generators, root, job, current.clone());
                        running.push(Box::pin(async move { Finished::Job(Box::new(started.await)) }));
                    } else {
                        waiting.push_back(job);
                    }
                }
                queue = waiting;
            }
            for (job, outcome) in early {
                if stopped {
                    queue.push_back(job);
                    continue;
                }
                if !done(&job, &outcome) {
                    stopped = true;
                }
                results.push((job, outcome));
            }
            let freed = std::pin::pin!(places.freed.notified());
            let finished = match select(running.next(), freed).await {
                Either::Left((Some(next), _)) => next,
                Either::Left((None, _)) => break,
                Either::Right(_) => continue,
            };
            match finished {
                Finished::Prepared(instance, mut result) => {
                    places.remove(&instance);
                    let mut caches = caches.lock().expect("plugin caches");
                    if let Some(cache) = caches.get_mut(&instance)
                        && let Err(message) = cache.finish(result.is_ok())
                    {
                        result = match result {
                            Err(mut failed) => {
                                failed.failure.message = format!("{}; {message}", failed.failure.message);
                                Err(failed)
                            },
                            Ok(()) => Err(PrepareFailure {
                                failure: Diagnostic::error("generate::prepare", format!("{instance}: {message}")),
                                secrets: Vec::new(),
                            }),
                        };
                    }
                    preparing.insert(instance, Preparing::Done(result));
                },
                Finished::Job(finished) => {
                    let (job, outcome) = *finished;
                    places.remove(&job.instance);
                    if !done(&job, &outcome) {
                        stopped = true;
                    }
                    results.push((job, outcome));
                },
            }
        }
        results.extend(queue.into_iter().map(|job| (job, Outcome::Skipped)));
    });
    caches.lock().expect("plugin caches").clear();
    streams.lock().expect("streams").clear();
    for plugin in generators.values() {
        let _ = uninstall(plugin);
    }
    results
}

/// A finished future of the run: a job, or the `prepare` call of an instance.
enum Finished {
    Job(Box<(Job, Outcome)>),
    Prepared(String, Result<(), PrepareFailure>),
}

enum Preparing {
    Running,
    Done(Result<(), PrepareFailure>),
}

/// Why `prepare` failed, with the secrets that a refused request used, as for a job.
#[derive(Clone)]
struct PrepareFailure {
    failure: Diagnostic,
    secrets: Vec<String>,
}

impl PrepareFailure {
    /// The outcome of a job whose instance failed in `prepare`.
    fn outcome(&self) -> Outcome {
        if self.secrets.is_empty() {
            Outcome::Failed(self.failure.clone())
        } else {
            Outcome::Unauthorised {
                message: self.failure.message.clone(),
                secrets: self.secrets.clone(),
            }
        }
    }
}

/// Calls `prepare(vars)` of a plugin instance as a coroutine, with the limits of one job. Its
/// `janky.cache` writes go into one transaction, which the run ends when the call ends.
fn start_prepare(
    plugins: &Plugins,
    plugin: &Plugin,
    current: Current,
) -> Pin<Box<dyn Future<Output = Result<(), PrepareFailure>>>> {
    let sandbox = plugins.sandbox.clone();
    let label = format!("{}: `prepare`", plugin.name);
    let (source, chunk, instance) = (plugin.source.clone(), plugin.chunk.clone(), plugin.name.clone());
    let called = sandbox.guarded(&label, || {
        set_variables(&sandbox, &plugin.env, &plugin.values)?;
        let vars = crate::host::to_lua(&sandbox.lua, &plugin.values)?;
        let prepare = plugin.prepare.clone().expect("prepare");
        Ok(prepare.call_async::<MultiValue>(vars))
    });
    let failed = move |mut failure: Diagnostic, secrets: Vec<String>| {
        failure.message = format!("{instance}: `prepare` failed: {}", failure.message);
        PrepareFailure { failure, secrets }
    };
    let call = match called {
        Ok(call) => call,
        Err(error) => {
            let failure = failed(lua_error(&source, &chunk, &error), Vec::new());
            return Box::pin(async move { Err(failure) });
        },
    };
    let context = JobContext { prepare: true, ..JobContext::default() };
    let refused = context.refused.clone();
    let guarded = Guarded {
        sandbox,
        label,
        context,
        current,
        used: 0,
        inner: Some(Box::pin(call)),
    };
    Box::pin(async move {
        let failure = match tokio::time::timeout(JOB_TIME_LIMIT, guarded).await {
            Ok(Ok(_)) => return Ok(()),
            Ok(Err(error)) => lua_error(&source, &chunk, &error),
            Err(_) => {
                Diagnostic::error("generate::time", format!("it ran longer than {} seconds", JOB_TIME_LIMIT.as_secs()))
            },
        };
        let secrets = refused.lock().expect("refused secrets").clone();
        Err(failed(failure, secrets))
    })
}

fn start(
    plugins: &Plugins,
    generators: &HashMap<&str, &Plugin>,
    root: &Path,
    job: Job,
    current: Current,
) -> Pin<Box<dyn Future<Output = (Job, Outcome)>>> {
    let Some(plugin) = generators.get(job.instance.as_str()) else {
        let failure = Diagnostic::error("generate::instance", format!("`{}` has no `generate` function", job.instance));
        return Box::pin(async move { (job, Outcome::Failed(failure)) });
    };
    let sandbox = plugins.sandbox.clone();
    let label = format!("{}: `generate`", job.instance);
    let (source, chunk) = (plugin.source.clone(), plugin.chunk.clone());
    let root = root.to_path_buf();
    let prepared = sandbox.guarded(&label, || {
        set_variables(&sandbox, &plugin.env, &plugin.values)?;
        let table = sandbox.lua.create_table()?;
        table.raw_set(
            "input",
            crate::host::to_lua(&sandbox.lua, &minijinja::Value::from(minijinja::value::Serde(&job.input)))?,
        )?;
        table.raw_set("kind", job.kind.name())?;
        let vars = crate::host::to_lua(&sandbox.lua, &plugin.values)?;
        let generate = plugin.generate.clone().expect("generator");
        Ok(generate.call_async::<MultiValue>((table, vars)))
    });
    let call = match prepared {
        Ok(call) => call,
        Err(error) => {
            let failure = lua_error(&source, &chunk, &error);
            return Box::pin(async move { (job, Outcome::Failed(failure)) });
        },
    };
    let context = JobContext::default();
    let refused = context.refused.clone();
    let guarded = Guarded {
        sandbox,
        label,
        context,
        current,
        used: 0,
        inner: Some(Box::pin(call)),
    };
    Box::pin(async move {
        let outcome = match tokio::time::timeout(JOB_TIME_LIMIT, guarded).await {
            Ok(Ok(values)) => finish(&root, &job, values),
            Ok(Err(error)) => Outcome::Failed(lua_error(&source, &chunk, &error)),
            Err(_) => Outcome::Failed(Diagnostic::error(
                "generate::time",
                format!("{}: the job ran longer than {} seconds", job.instance, JOB_TIME_LIMIT.as_secs()),
            )),
        };
        let secrets = refused.lock().expect("refused secrets").clone();
        let outcome = match outcome {
            Outcome::Refused(message) if !secrets.is_empty() => Outcome::Unauthorised { message, secrets },
            Outcome::Failed(failure) if !secrets.is_empty() => Outcome::Unauthorised {
                message: failure.message.clone(),
                secrets,
            },
            other => other,
        };
        (job, outcome)
    })
}

fn finish(root: &Path, job: &Job, values: MultiValue) -> Outcome {
    let mut values = values.into_iter();
    let fail = |message: String| {
        Outcome::Failed(Diagnostic::error("generate::result", format!("{}: `generate` {message}", job.instance)))
    };
    let result = match values.next() {
        Some(Value::Table(result)) => result,
        Some(Value::Nil) | None => {
            let message = match values.next() {
                Some(Value::String(message)) => message.to_string_lossy(),
                _ => "no reason given".to_string(),
            };
            return Outcome::Refused(message);
        },
        Some(_) => return fail("must return a table, or nil and a message".into()),
    };
    let cost = match result.raw_get::<Value>("cost") {
        Ok(Value::Nil) => None,
        Ok(Value::Integer(cost)) if cost >= 0 => Some(cost as f64),
        Ok(Value::Number(cost)) if cost.is_finite() && cost >= 0.0 => Some(cost),
        _ => return fail("must return `cost` as a number, 0 or more".into()),
    };
    if job.kind == Kind::Data {
        return finish_data(root, job, &result, cost).unwrap_or_else(&fail);
    }
    if !matches!(result.raw_get::<Value>("value"), Ok(Value::Nil)) {
        return fail("returned `value` for a media job: return `data` and `ext`".into());
    }
    let data = match result.raw_get::<Value>("data") {
        Ok(Value::String(data)) => data.as_bytes().to_vec(),
        _ => return fail("must return `data` as a string".into()),
    };
    let ext = match result.raw_get::<Value>("ext") {
        Ok(Value::String(ext)) if valid_ext(&ext.to_string_lossy()) => ext.to_string_lossy(),
        _ => return fail("must return `ext`: 1 to 10 lower-case letters or digits, not `json`".into()),
    };
    if job.ext.as_ref().is_some_and(|planned| *planned != ext) {
        return fail(format!("returned `{ext}`, but `plan` gave `{}`", job.ext.as_deref().unwrap_or_default()));
    }
    let meta = Meta {
        ext,
        instance: job.instance.clone(),
        cost,
    };
    match cache::store(root, &job.hash, &data, &meta) {
        Ok(file) => Outcome::Stored { file, cost },
        Err(message) => Outcome::Failed(Diagnostic::error("generate::store", message)),
    }
}

/// The longest warning or info that a data result can carry, in bytes.
pub const WARNING_LIMIT: usize = 2000;

fn finish_data(root: &Path, job: &Job, result: &Table, cost: Option<f64>) -> Result<Outcome, String> {
    if !matches!(result.raw_get::<Value>("data"), Ok(Value::Nil))
        || !matches!(result.raw_get::<Value>("ext"), Ok(Value::Nil))
    {
        return Err("returned `data` or `ext` for a data job: return `value`".into());
    }
    let value = match result.raw_get::<Value>("value") {
        Ok(value @ Value::Table(_)) => crate::host::lua_to_json(&value)?,
        _ => return Err("must return `value` as a table for a data job".into()),
    };
    let value = match value {
        serde_json::Value::Array(items) if items.is_empty() => serde_json::Value::Object(serde_json::Map::new()),
        value @ serde_json::Value::Object(_) => value,
        _ => return Err("must return `value` as a table with names, not a list".into()),
    };
    if value.to_string().len() > cache::VALUE_LIMIT {
        return Err(format!("returned a `value` longer than {} bytes as JSON", cache::VALUE_LIMIT));
    }
    let warning = note(result, "warning")?;
    let info = note(result, "info")?;
    let text = value.to_string();
    let secret = |text: &str| crate::diagnostic::redacted(text) != text;
    if secret(&text) || warning.as_deref().is_some_and(secret) || info.as_deref().is_some_and(secret) {
        return Err(
            "returned a secret value in `value`, `warning` or `info`, so janky does not store the result".into()
        );
    }
    let data = cache::Data {
        instance: job.instance.clone(),
        value,
        warning,
        info,
        cost,
    };
    Ok(match cache::store_data(root, &job.hash, &data) {
        Ok(file) => Outcome::Stored { file, cost },
        Err(message) => Outcome::Failed(Diagnostic::error("generate::store", message)),
    })
}

/// Reads the text `key` (`warning` or `info`) of a data result.
fn note(result: &Table, key: &str) -> Result<Option<String>, String> {
    match result.raw_get::<Value>(key) {
        Ok(Value::Nil) => Ok(None),
        Ok(Value::String(text)) => {
            let text = text.to_str().map_err(|_| format!("must return `{key}` as UTF-8 text"))?.to_string();
            if text.len() > WARNING_LIMIT || text.chars().any(|c| c.is_control() && c != '\n') {
                return Err(format!(
                    "must return `{key}` as text of at most {WARNING_LIMIT} bytes, without control characters"
                ));
            }
            Ok(Some(text))
        },
        _ => Err(format!("must return `{key}` as a string")),
    }
}

fn host_table(plugin: &Plugin) -> mlua::Result<Option<Table>> {
    match plugin.env.raw_get::<Value>("janky")? {
        Value::Table(janky) => Ok(Some(janky)),
        _ => Ok(None),
    }
}

/// The state of a run that the host functions of every instance share.
#[derive(Clone)]
struct Shared {
    http: Arc<dyn Http>,
    current: Current,
    places: Places,
    caches: Caches,
    streams: Streams,
}

fn install(sandbox: &Sandbox, plugin: &Plugin, root: &Path, shared: Shared) -> mlua::Result<()> {
    let Shared { http, current, places, caches, streams } = shared;
    let Some(janky) = host_table(plugin)? else { return Ok(()) };
    let lua = &sandbox.lua;
    let instance = plugin.name.clone();
    let names = plugin.secrets.clone();
    let checking = current.clone();
    let secret = lua.create_function(move |_, (name, prefix): (String, Option<String>)| {
        if checking.lock().expect("current job").is_none() {
            return Err(mlua::Error::runtime("janky.secret works only inside `generate` or `prepare`"));
        }
        if !names.contains(&name) {
            return Err(mlua::Error::runtime(format!("the plugin does not list the secret `{name}` in `secrets`")));
        }
        let prefix = prefix.unwrap_or_default();
        if prefix.len() > 64 || prefix.chars().any(char::is_control) {
            return Err(mlua::Error::runtime("a secret prefix has at most 64 characters and no control characters"));
        }
        Ok(Secret { name, prefix })
    })?;
    let waiting = current.clone();
    let wait = lua.create_async_function(move |_, ms: f64| {
        let inside = waiting.lock().expect("current job").is_some();
        async move {
            if !inside {
                return Err(mlua::Error::runtime("janky.wait works only inside `generate` or `prepare`"));
            }
            let ms = if ms.is_finite() { ms.clamp(0.0, WAIT_LIMIT.as_millis() as f64) } else { 0.0 };
            tokio::time::sleep(Duration::from_millis(ms as u64)).await;
            Ok(())
        }
    })?;
    let caching = (current.clone(), caches, instance.clone(), root.to_path_buf());
    let reading = caching.clone();
    let get = lua.create_function(move |lua, key: String| {
        let (current, caches, instance, root) = &reading;
        let caches = open_cache(current, caches, instance, root)?;
        let value = caches.get(instance.as_str()).expect("opened cache").get(&key);
        drop(caches);
        match value.map_err(mlua::Error::runtime)? {
            Some(json) => crate::host::to_lua(lua, &minijinja::Value::from(minijinja::value::Serde(&json))),
            None => Ok(Value::Nil),
        }
    })?;
    let put = lua.create_function(move |_, (key, value): (String, Value)| {
        let (current, caches, instance, root) = &caching;
        let json = match value {
            Value::Nil => None,
            value => Some(crate::host::lua_to_json(&value).map_err(mlua::Error::runtime)?),
        };
        let mut caches = open_cache(current, caches, instance, root)?;
        let cache = caches.get_mut(instance.as_str()).expect("opened cache");
        cache.put(&key, json.as_ref()).map_err(mlua::Error::runtime)
    })?;
    let cache_table = lua.create_table()?;
    cache_table.raw_set("get", get)?;
    cache_table.raw_set("put", put)?;
    janky.raw_set("cache", cache_table)?;
    janky.raw_set(
        "fs",
        fs_table(lua, &current, crate::fs::Grants::new(plugin.read.clone(), root, &instance), streams)?,
    )?;
    let (downloading, root) = ((current.clone(), http.clone(), instance.clone(), places.clone()), root.to_path_buf());
    let request = lua.create_async_function(move |lua, spec: Table| {
        let context = current.lock().expect("current job").clone();
        let parsed = parse_request(&spec, false);
        let http = http.clone();
        let instance = instance.clone();
        let places = places.clone();
        async move {
            let (context, request, used) = begin(context, parsed, "janky.http.request")?;
            let pause = Pause { places, instance: instance.clone() };
            let response = http.send(&instance, request, pause).await.map_err(mlua::Error::runtime)?;
            *context.refused.lock().expect("refused secrets") =
                if matches!(response.status, 401 | 403) { used } else { Vec::new() };
            let table = lua.create_table()?;
            table.raw_set("status", response.status)?;
            table.raw_set("headers", header_table(&lua, &response.headers)?)?;
            table.raw_set("body", lua.create_string(&response.body)?)?;
            Ok(table)
        }
    })?;
    let download = lua.create_async_function(move |lua, spec: Table| {
        let (current, http, instance, places) = &downloading;
        let context = current.lock().expect("current job").clone();
        let parsed = parse_request(&spec, true);
        let file = match spec.raw_get::<Value>("file") {
            Ok(Value::Nil) => Err("a download needs `file`, the name of the file to write".to_string()),
            Ok(value) => text(value, "`file`"),
            Err(error) => Err(error.to_string()),
        };
        let (http, instance, places, root) = (http.clone(), instance.clone(), places.clone(), root.clone());
        async move {
            let (context, request, used) = begin(context, parsed, "janky.http.download")?;
            let target =
                file.and_then(|file| cache::plugin_file(&root, &instance, &file)).map_err(mlua::Error::runtime)?;
            let pause = Pause { places, instance: instance.clone() };
            let downloaded = http.download(&instance, request, target, pause).await.map_err(mlua::Error::runtime)?;
            *context.refused.lock().expect("refused secrets") =
                if matches!(downloaded.status, 401 | 403) { used } else { Vec::new() };
            let table = lua.create_table()?;
            table.raw_set("status", downloaded.status)?;
            table.raw_set("headers", header_table(&lua, &downloaded.headers)?)?;
            table.raw_set("size", downloaded.size)?;
            Ok(table)
        }
    })?;
    let http_table = lua.create_table()?;
    http_table.raw_set("request", request)?;
    http_table.raw_set("download", download)?;
    janky.raw_set("http", http_table)?;
    janky.raw_set("secret", secret)?;
    janky.raw_set("wait", wait)
}

/// Checks that a job is running and may send another request, and returns its context, the
/// parsed request and the names of the secrets that the request uses.
fn begin(
    context: Option<JobContext>,
    parsed: Result<Request, String>,
    function: &str,
) -> mlua::Result<(JobContext, Request, Vec<String>)> {
    let context =
        context.ok_or_else(|| mlua::Error::runtime(format!("{function} works only inside `generate` or `prepare`")))?;
    let request = parsed.map_err(mlua::Error::runtime)?;
    if context.requests.fetch_add(1, Ordering::Relaxed) >= REQUEST_LIMIT {
        return Err(mlua::Error::runtime(format!("a job may send at most {REQUEST_LIMIT} requests")));
    }
    let used = request
        .headers
        .iter()
        .filter_map(|(_, value)| match value {
            HeaderText::Secret { name, .. } => Some(name.clone()),
            HeaderText::Text(_) => None,
        })
        .collect();
    Ok((context, request, used))
}

/// The response headers as a Lua table, with lower-case names and repeated headers joined with
/// `, `.
fn header_table(lua: &mlua::Lua, headers: &[(String, String)]) -> mlua::Result<Table> {
    let mut joined: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for (name, value) in headers {
        joined
            .entry(name.to_ascii_lowercase())
            .and_modify(|all| {
                all.push_str(", ");
                all.push_str(value);
            })
            .or_insert_with(|| value.clone());
    }
    let table = lua.create_table()?;
    for (name, value) in joined {
        table.raw_set(name, value)?;
    }
    Ok(table)
}

fn uninstall(plugin: &Plugin) -> mlua::Result<()> {
    let Some(janky) = host_table(plugin)? else { return Ok(()) };
    janky.raw_set("http", Value::Nil)?;
    janky.raw_set("cache", Value::Nil)?;
    janky.raw_set("fs", Value::Nil)?;
    janky.raw_set("secret", Value::Nil)?;
    janky.raw_set("wait", Value::Nil)
}

/// Makes the `janky.fs` table of one instance. Each function works only while a job of the run
/// is being resumed. The iterator of `json_items` yields before each item, so that each item
/// ends a resume and the limits for each resume apply, whatever Tokio's own budget does.
fn fs_table(lua: &mlua::Lua, current: &Current, grants: crate::fs::Grants, streams: Streams) -> mlua::Result<Table> {
    let inside = |current: &Current| {
        if current.lock().expect("current job").is_none() {
            return Err(mlua::Error::runtime("janky.fs works only inside `generate` or `prepare`"));
        }
        Ok(())
    };
    let table = lua.create_table()?;
    let (checking, reading) = (current.clone(), grants.clone());
    table.raw_set(
        "read",
        lua.create_function(move |lua, path: String| {
            inside(&checking)?;
            lua.create_string(crate::fs::read(&reading, &path).map_err(mlua::Error::runtime)?)
        })?,
    )?;
    let (checking, listing) = (current.clone(), grants.clone());
    table.raw_set(
        "list",
        lua.create_function(move |lua, path: String| {
            inside(&checking)?;
            lua.create_sequence_from(crate::fs::list(&listing, &path).map_err(mlua::Error::runtime)?)
        })?,
    )?;
    let (checking, zipped) = (current.clone(), grants.clone());
    table.raw_set(
        "zip_entries",
        lua.create_function(move |lua, path: String| {
            inside(&checking)?;
            lua.create_sequence_from(crate::fs::zip_entries(&zipped, &path).map_err(mlua::Error::runtime)?)
        })?,
    )?;
    let checking = current.clone();
    table.raw_set(
        "json_items",
        lua.create_function(move |lua, (path, pointer, options): (String, String, Option<Table>)| {
            inside(&checking)?;
            let entry = match options {
                Some(options) => options.raw_get::<Option<String>>("entry")?,
                None => None,
            };
            let receiver = crate::fs::json_items(&grants, &path, &pointer, entry).map_err(mlua::Error::runtime)?;
            let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
            streams.lock().expect("streams").push(receiver.clone());
            let receiver = Arc::downgrade(&receiver);
            let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let checking = checking.clone();
            lua.create_async_function(move |lua, ()| {
                let inside = checking.lock().expect("current job").is_some();
                let (receiver, finished) = (receiver.upgrade(), finished.clone());
                async move {
                    if !inside {
                        return Err(mlua::Error::runtime("janky.fs works only inside `generate` or `prepare`"));
                    }
                    if finished.load(Ordering::Relaxed) {
                        return Ok(Value::Nil);
                    }
                    tokio::task::yield_now().await;
                    let receiver =
                        receiver.ok_or_else(|| mlua::Error::runtime("the JSON stream ended with its run"))?;
                    let message = receiver.lock().await.recv().await;
                    match message {
                        Some(crate::fs::Message::Item(item)) => {
                            crate::host::to_lua(&lua, &minijinja::Value::from(minijinja::value::Serde(&item)))
                        },
                        Some(crate::fs::Message::End) => {
                            finished.store(true, Ordering::Relaxed);
                            Ok(Value::Nil)
                        },
                        Some(crate::fs::Message::Error(message)) => {
                            finished.store(true, Ordering::Relaxed);
                            Err(mlua::Error::runtime(message))
                        },
                        None => Err(mlua::Error::runtime("the JSON stream stopped early")),
                    }
                }
            })
        })?,
    )?;
    Ok(table)
}

/// The `json_items` streams of a run. The run owns them and drops them at its end, which stops
/// their threads; the Lua iterators hold only weak references.
type Streams = Arc<Mutex<Vec<Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<crate::fs::Message>>>>>>;

/// The plugin caches that a run has opened, by instance name. A cache opens on first use.
type Caches = Arc<Mutex<HashMap<String, crate::plugin_cache::PluginCache>>>;

fn open_cache<'a>(
    current: &Current,
    caches: &'a Caches,
    instance: &str,
    root: &Path,
) -> mlua::Result<std::sync::MutexGuard<'a, HashMap<String, crate::plugin_cache::PluginCache>>> {
    let preparing = match current.lock().expect("current job").as_ref() {
        None => return Err(mlua::Error::runtime("janky.cache works only inside `generate` or `prepare`")),
        Some(context) => context.prepare,
    };
    let mut caches = caches.lock().expect("plugin caches");
    if !caches.contains_key(instance) {
        let cache = crate::plugin_cache::PluginCache::open(root, instance).map_err(mlua::Error::runtime)?;
        caches.insert(instance.to_string(), cache);
    }
    if preparing {
        caches.get_mut(instance).expect("opened cache").begin().map_err(mlua::Error::runtime)?;
    }
    Ok(caches)
}

fn text(value: Value, what: &str) -> Result<String, String> {
    match value {
        Value::String(text) => {
            text.to_str().map(|text| text.to_string()).map_err(|_| format!("{what} must be UTF-8 text"))
        },
        _ => Err(format!("{what} must be a string")),
    }
}

fn parse_request(spec: &Table, download: bool) -> Result<Request, String> {
    let method = match spec.raw_get::<Value>("method").map_err(|error| error.to_string())? {
        Value::Nil => "GET".to_string(),
        value => text(value, "`method`")?.to_ascii_uppercase(),
    };
    if !METHODS.contains(&method.as_str()) {
        return Err(format!("`method` must be one of {}", METHODS.join(", ")));
    }
    let url = text(spec.raw_get::<Value>("url").map_err(|error| error.to_string())?, "`url`")?;
    let mut headers = Vec::new();
    match spec.raw_get::<Value>("headers").map_err(|error| error.to_string())? {
        Value::Nil => {},
        Value::Table(table) => {
            for pair in table.pairs::<Value, Value>() {
                let (name, value) = pair.map_err(|error| error.to_string())?;
                let name = text(name, "a header name")?;
                let value = match value {
                    Value::UserData(data) => match data.borrow::<Secret>() {
                        Ok(secret) => HeaderText::Secret {
                            name: secret.name.clone(),
                            prefix: secret.prefix.clone(),
                        },
                        Err(_) => return Err(format!("the header `{name}` must be a string or a secret")),
                    },
                    other => HeaderText::Text(text(other, &format!("the header `{name}`"))?),
                };
                let broken = matches!(&value, HeaderText::Text(value) if value.contains(['\r', '\n']));
                if name.is_empty() || name.contains(['\r', '\n', ':']) || broken {
                    return Err(format!("the header `{name}` has a line break or a colon in a wrong place"));
                }
                let lower = name.to_ascii_lowercase();
                if FORBIDDEN_HEADERS.contains(&lower.as_str()) || lower.starts_with("proxy-") {
                    return Err(format!("a plugin cannot set the header `{name}`"));
                }
                if headers.len() >= 64 {
                    return Err("a request may have at most 64 headers".into());
                }
                headers.push((name, value));
            }
            headers.sort();
        },
        _ => return Err("`headers` must be a table".into()),
    }
    let body = match spec.raw_get::<Value>("body").map_err(|error| error.to_string())? {
        Value::Nil => Vec::new(),
        Value::String(body) => body.as_bytes().to_vec(),
        _ => return Err("`body` must be a string".into()),
    };
    let keys: &[&str] =
        if download { &["method", "url", "headers", "body", "file"] } else { &["method", "url", "headers", "body"] };
    for key in spec.pairs::<Value, Value>().filter_map(Result::ok).map(|(key, _)| key) {
        let known = matches!(&key, Value::String(name) if keys.contains(&name.to_string_lossy().as_str()));
        if !known && download {
            return Err("a download has only `method`, `url`, `headers`, `body` and `file`".into());
        }
        if !known {
            return Err("a request has only `method`, `url`, `headers` and `body`".into());
        }
    }
    Ok(Request { method, url, headers, body })
}
