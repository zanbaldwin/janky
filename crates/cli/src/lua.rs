//! The restricted Lua 5.4 state that runs every plugin.
//!
//! A plugin script is untrusted. [`Sandbox`] loads only the `string`, `table`, `math` and `utf8`
//! libraries, and removes `load`, `dofile`, `loadfile`, `collectgarbage`, `math.random` and
//! `math.randomseed`. So a plugin has no `io`, `os`, `debug`, `require` or `coroutine`. The
//! string metatable is locked. A small Lua guard replaces three globals:
//!
//! - `pcall` and `xpcall` check the instruction limit after the protected call returns. A plugin
//!   therefore cannot catch the limit error. The `xpcall` message handler runs after that check,
//!   under the instruction count.
//! - `setmetatable` refuses a metatable with `__gc`. A finaliser could run plugin code outside a
//!   call, where no limit applies.
//!
//! Each call has three limits. An instruction count stops plain Lua loops after
//! [`INSTRUCTION_LIMIT`]. Lua allocations stop at [`MEMORY_LIMIT`]. A watchdog thread catches
//! slow C functions (for example a pattern match), which the instruction count does not see. It
//! acts when one call runs longer than [`TIME_LIMIT`]. The instruction count and the watchdog
//! start again at each [`Sandbox::begin`], so the limits apply to each call, not to the whole
//! build.
//!
//! `print` (and `janky.log`, the same function) writes to an in-memory log that keeps the last
//! [`LOG_LIMIT`] bytes. [`crate::host`] adds the `janky` table to the globals. [`crate::plugin`]
//! gives each plugin its own copy of the globals as its environment.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mlua::chunk::ChunkMode;
use mlua::{FromLuaMulti, Function, HookTriggers, IntoLuaMulti, Lua, LuaOptions, StdLib, VmState};

/// The most memory in bytes that the Lua state can allocate, for all plugins together.
pub const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
/// The most Lua instructions in one call. The count is checked every 10,000 instructions.
pub const INSTRUCTION_LIMIT: u64 = 10_000_000;
/// The most bytes that the plugin log keeps. Older lines are dropped first.
pub const LOG_LIMIT: usize = 1024 * 1024;
/// The longest time that one call can run before the watchdog acts.
pub const TIME_LIMIT: Duration = Duration::from_secs(10);
const HOOK_STEP: u32 = 10_000;

const GUARD: &str = r#"
local check, raw_pcall, pack, unpack, raw_setmetatable, rawget, type, error = ...
pcall = function(...)
  local result = pack(raw_pcall(...))
  check()
  return unpack(result, 1, result.n)
end
xpcall = function(f, handler, ...)
  local result = pack(raw_pcall(f, ...))
  check()
  if result[1] then
    return unpack(result, 1, result.n)
  end
  local _, value = raw_pcall(handler, result[2])
  check()
  return false, value
end
setmetatable = function(t, mt)
  if type(mt) == "table" and rawget(mt, "__gc") ~= nil then
    error("plugins cannot use __gc metamethods", 2)
  end
  return raw_setmetatable(t, mt)
end
"#;

/// What the watchdog does when a call runs too long. It gets the label of the call.
///
/// The watchdog runs on its own thread and cannot stop Lua. The action must therefore end the
/// call by other means. [`Sandbox::new`] uses an action that stops the process.
pub type TimeoutAction = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Default)]
struct Log {
    lines: VecDeque<String>,
    bytes: usize,
    dropped: usize,
}

impl Log {
    fn push(&mut self, mut line: String) {
        if line.len() > LOG_LIMIT {
            let mut cut = line.len() - LOG_LIMIT;
            while !line.is_char_boundary(cut) {
                cut += 1;
            }
            line = line[cut..].to_string();
        }
        self.bytes += line.len();
        self.lines.push_back(line);
        while self.bytes > LOG_LIMIT {
            let old = self.lines.pop_front().expect("line");
            self.bytes -= old.len();
            self.dropped += 1;
        }
    }

    fn take(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        if self.dropped > 0 {
            out.push(format!("({} earlier lines dropped: the log keeps the last {LOG_LIMIT} bytes)", self.dropped));
        }
        out.extend(self.lines.drain(..));
        *self = Log::default();
        out
    }
}

fn log_line(tostring: &Function, values: &mlua::MultiValue) -> mlua::Result<String> {
    let texts = values
        .iter()
        .map(|value| match value {
            mlua::Value::String(text) => Ok(text.clone()),
            other => tostring.call::<mlua::LuaString>(other.clone()),
        })
        .collect::<mlua::Result<Vec<_>>>()?;
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let mut size = 0;
    for (index, text) in texts.iter().enumerate().rev() {
        if size >= LOG_LIMIT {
            break;
        }
        let bytes = text.as_bytes();
        let mut chunk = bytes[bytes.len().saturating_sub(LOG_LIMIT - size)..].to_vec();
        if index > 0 {
            chunk.insert(0, b'\t');
        }
        size += chunk.len();
        chunks.push(chunk);
    }
    chunks.reverse();
    let line = chunks.concat();
    let start = if size >= LOG_LIMIT {
        line.iter().take(3).take_while(|byte| (0x80..0xc0).contains(*byte)).count()
    } else {
        0
    };
    Ok(String::from_utf8_lossy(&line[start..]).into_owned())
}

struct Running {
    since: Instant,
    label: String,
}

/// The one Lua state of a janky run, with its limits, its watchdog and its log.
///
/// Run plugin code only through [`exec`](Self::exec), [`exec_in`](Self::exec_in),
/// [`call`](Self::call) or [`guarded`](Self::guarded). These start the limits for the call and
/// give it a label, which the timeout message shows.
pub struct Sandbox {
    /// The Lua state. Code that runs through it directly has no watchdog and no fresh
    /// instruction count.
    pub lua: Lua,
    counter: Arc<AtomicU64>,
    exceeded: Arc<AtomicBool>,
    log: Arc<Mutex<Log>>,
    running: Arc<Mutex<Option<Running>>>,
}

fn limit_message() -> String {
    format!("the plugin ran more than {INSTRUCTION_LIMIT} instructions in one call")
}

fn limit_error() -> mlua::Error {
    mlua::Error::runtime(limit_message())
}

impl Sandbox {
    /// Makes a sandbox whose watchdog prints an error and stops the process with exit code 1
    /// when a call runs longer than [`TIME_LIMIT`].
    pub fn new() -> mlua::Result<Self> {
        Self::with_watchdog(
            TIME_LIMIT,
            Arc::new(|label: &str| {
                eprintln!(
                    "error: the plugin call {} ran longer than {} seconds",
                    crate::diagnostic::visible(label),
                    TIME_LIMIT.as_secs()
                );
                std::process::exit(1);
            }),
        )
    }

    /// Makes a sandbox with a custom time limit and timeout action. Tests use it.
    ///
    /// The watchdog thread checks the running call every 50 ms. It calls `on_timeout` once, for
    /// the first call that runs longer than `limit`, and then stops. It also stops when the
    /// sandbox is dropped.
    pub fn with_watchdog(limit: Duration, on_timeout: TimeoutAction) -> mlua::Result<Self> {
        let lua = Lua::new_with(StdLib::STRING | StdLib::TABLE | StdLib::MATH | StdLib::UTF8, LuaOptions::new())?;
        let globals = lua.globals();
        for name in ["dofile", "loadfile", "load", "collectgarbage"] {
            globals.set(name, mlua::Nil)?;
        }
        let math: mlua::Table = globals.get("math")?;
        for name in ["random", "randomseed"] {
            math.set(name, mlua::Nil)?;
        }

        let log = Arc::new(Mutex::new(Log::default()));
        let sink = log.clone();
        let tostring: Function = globals.get("tostring")?;
        let print = lua.create_function(move |_, values: mlua::MultiValue| {
            let line = log_line(&tostring, &values)?;
            sink.lock().expect("plugin log").push(line);
            Ok(())
        })?;
        globals.set("print", print.clone())?;
        crate::host::install(&lua, print)?;

        let exceeded = Arc::new(AtomicBool::new(false));
        let flag = exceeded.clone();
        let check =
            lua.create_function(move |_, ()| if flag.load(Ordering::Relaxed) { Err(limit_error()) } else { Ok(()) })?;
        let table: mlua::Table = globals.get("table")?;
        lua.load(GUARD).set_name("=janky").call::<()>((
            check,
            globals.get::<Function>("pcall")?,
            table.get::<Function>("pack")?,
            table.get::<Function>("unpack")?,
            globals.get::<Function>("setmetatable")?,
            globals.get::<Function>("rawget")?,
            globals.get::<Function>("type")?,
            globals.get::<Function>("error")?,
        ))?;

        lua.load("getmetatable('').__metatable = false").set_name("=janky").exec()?;
        lua.set_memory_limit(MEMORY_LIMIT)?;
        let counter = Arc::new(AtomicU64::new(0));
        let hook = |count: Arc<AtomicU64>, flag: Arc<AtomicBool>| {
            move |_: &Lua, debug: &mlua::debug::Debug| {
                let used = count.fetch_add(u64::from(HOOK_STEP), Ordering::Relaxed) + u64::from(HOOK_STEP);
                if used > INSTRUCTION_LIMIT || flag.load(Ordering::Relaxed) {
                    flag.store(true, Ordering::Relaxed);
                    let place = match (debug.source().short_src, debug.current_line()) {
                        (Some(source), Some(line)) => format!("{source}:{line}: "),
                        _ => String::new(),
                    };
                    return Err(mlua::Error::runtime(format!("{place}{}", limit_message())));
                }
                Ok(VmState::Continue)
            }
        };
        let triggers = HookTriggers::new().every_nth_instruction(HOOK_STEP);
        lua.set_hook(triggers, hook(counter.clone(), exceeded.clone()))?;
        lua.set_global_hook(triggers, hook(counter.clone(), exceeded.clone()))?;

        let running: Arc<Mutex<Option<Running>>> = Arc::new(Mutex::new(None));
        let watched = Arc::downgrade(&running);
        std::thread::spawn(move || {
            while let Some(running) = watched.upgrade() {
                let late = running
                    .lock()
                    .expect("watchdog")
                    .as_ref()
                    .filter(|call| call.since.elapsed() > limit)
                    .map(|call| call.label.clone());
                drop(running);
                if let Some(label) = late {
                    on_timeout(&label);
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        Ok(Self { lua, counter, exceeded, log, running })
    }

    /// Starts the limits for a call: it sets the instruction count to zero, clears the limit
    /// error and starts the watchdog clock.
    ///
    /// Each `begin` needs an [`end`](Self::end). The generator runner calls the pair around each
    /// resume of a job, so that time spent waiting for a host function does not count.
    pub fn begin(&self, label: &str) {
        self.counter.store(0, Ordering::Relaxed);
        self.exceeded.store(false, Ordering::Relaxed);
        *self.running.lock().expect("watchdog") = Some(Running {
            since: Instant::now(),
            label: label.to_string(),
        });
    }

    /// The instructions counted since the last [`begin`](Self::begin), in steps of 10,000.
    pub fn used(&self) -> u64 {
        self.counter.load(Ordering::Relaxed)
    }

    /// Stops the watchdog clock of the current call.
    pub fn end(&self) {
        *self.running.lock().expect("watchdog") = None;
    }

    /// Runs `run` between [`begin`](Self::begin) and [`end`](Self::end). Put all Lua work of a
    /// call inside, also the conversion of its arguments and the setup of its variables.
    pub fn guarded<R>(&self, label: &str, run: impl FnOnce() -> mlua::Result<R>) -> mlua::Result<R> {
        self.begin(label);
        let result = run();
        self.end();
        result
    }

    /// Runs `source` in the global environment and gives back its value.
    ///
    /// The chunk must be text, so precompiled bytecode is refused. Errors name the chunk as
    /// `name:line:`. `name` is also the label of the call.
    pub fn exec(&self, source: &str, name: &str) -> mlua::Result<mlua::Value> {
        self.guarded(name, || self.lua.load(source).set_name(format!("@{name}")).set_mode(ChunkMode::Text).eval())
    }

    /// Runs `source` like [`exec`](Self::exec), but with `env` as its global environment.
    ///
    /// Errors name the chunk as `chunk:line:`. Lua shortens long chunk names, so `chunk` is a
    /// short name. `label` names the call for the watchdog.
    pub fn exec_in(&self, source: &str, chunk: &str, label: &str, env: mlua::Table) -> mlua::Result<mlua::Value> {
        self.guarded(label, || {
            self.lua.load(source).set_name(format!("@{chunk}")).set_mode(ChunkMode::Text).set_environment(env).eval()
        })
    }

    /// Calls a Lua function inside [`guarded`](Self::guarded).
    pub fn call<R: FromLuaMulti>(&self, label: &str, function: &Function, args: impl IntoLuaMulti) -> mlua::Result<R> {
        self.guarded(label, || function.call(args))
    }

    /// Removes and gives back the log lines, oldest first. When the log dropped lines, the first
    /// line says how many.
    pub fn take_log(&self) -> Vec<String> {
        self.log.lock().expect("plugin log").take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(sandbox: &Sandbox, source: &str) -> String {
        sandbox.exec(source, "probe.lua").unwrap_err().to_string()
    }

    #[test]
    fn only_safe_libraries_are_available() {
        let sandbox = Sandbox::new().unwrap();
        for source in [
            "return io.open('/etc/hostname')",
            "return os.execute('id')",
            "return require('x')",
            "return dofile('/etc/hostname')",
            "return loadfile('/etc/hostname')",
            "return load('return 1')()",
            "return collectgarbage('count')",
            "return math.random(1, 6)",
            "return debug.getinfo(1)",
            "return coroutine.create(print)",
        ] {
            assert!(error(&sandbox, source).contains("attempt to"), "{source}");
        }
        let value: String = sandbox
            .lua
            .load("return string.upper('a') .. table.concat({'b', 'c'}) .. math.floor(2.5) .. utf8.char(129514)")
            .eval()
            .unwrap();
        assert_eq!(value, "Abc2🧪");
    }

    #[test]
    fn print_writes_to_the_log() {
        let sandbox = Sandbox::new().unwrap();
        sandbox.exec("print('⚛️🧪👩‍🔬', 1, true, nil)", "probe.lua").unwrap();
        assert_eq!(sandbox.take_log(), ["⚛️🧪👩‍🔬\t1\ttrue\tnil"]);
        assert!(sandbox.take_log().is_empty());
    }

    #[test]
    fn an_endless_loop_stops_and_the_limit_is_per_call() {
        let sandbox = Sandbox::new().unwrap();
        let message = error(&sandbox, "while true do end");
        assert!(message.contains("more than 10000000 instructions"), "{message}");
        sandbox
            .exec("function work() local n = 0 for i = 1, 1000000 do n = n + i end return n end", "probe.lua")
            .unwrap();
        let work: Function = sandbox.lua.globals().get("work").unwrap();
        for _ in 0..5 {
            let total: i64 = sandbox.call("work", &work, ()).unwrap();
            assert_eq!(total, 500_000_500_000);
        }
    }

    #[test]
    fn memory_is_limited() {
        let sandbox = Sandbox::new().unwrap();
        let message = error(&sandbox, "local t = {} for i = 1, 100 do t[i] = string.rep('x', 1024 * 1024) end");
        assert!(message.contains("memory"), "{message}");
        assert_eq!(sandbox.exec("return 1 + 1", "probe.lua").unwrap().as_integer(), Some(2));
    }

    #[test]
    fn errors_name_the_script_and_line() {
        let sandbox = Sandbox::new().unwrap();
        let message = error(&sandbox, "local x = 1\nreturn x.y");
        assert!(message.contains("probe.lua:2:"), "{message}");
    }

    #[test]
    fn pcall_cannot_catch_the_instruction_limit() {
        let sandbox = Sandbox::new().unwrap();
        let message = error(&sandbox, "while true do pcall(function() while true do end end) end");
        assert!(message.contains("more than 10000000 instructions"), "{message}");
        let message = error(&sandbox, "while true do xpcall(function() while true do end end, print) end");
        assert!(message.contains("more than 10000000 instructions"), "{message}");
        for source in [
            "xpcall(function() while true do end end, function() while true do end end)",
            "xpcall(function() error('x') end, function() while true do end end)",
        ] {
            let message = error(&sandbox, source);
            assert!(message.contains("more than 10000000 instructions"), "{message}");
        }
        let value = sandbox.exec(
            "local ok, e = xpcall(function() error({code = 3}) end, function(e) return e.code * 2 end) \
             local fine, a, b = xpcall(function(x, y) return x + y, 'b' end, print, 1, 2) \
             return tostring(ok) .. e .. tostring(fine) .. a .. b",
            "probe.lua",
        );
        assert_eq!(value.unwrap().to_string().unwrap(), "false6true3b");
        let value = sandbox.exec("local ok, e = pcall(error, {code = 7}) return tostring(ok) .. e.code", "probe.lua");
        assert_eq!(value.unwrap().to_string().unwrap(), "false7");
    }

    #[test]
    fn the_log_keeps_its_last_mebibyte() {
        let sandbox = Sandbox::new().unwrap();
        sandbox
            .exec(
                "local s = string.rep('x', 1024 * 1024) for i = 1, 5 do print(s) end print('the last line')",
                "probe.lua",
            )
            .unwrap();
        let log = sandbox.take_log();
        assert_eq!(
            log,
            [
                "(5 earlier lines dropped: the log keeps the last 1048576 bytes)",
                "the last line"
            ]
        );
        assert!(log.iter().map(String::len).sum::<usize>() <= LOG_LIMIT + 100);
        sandbox.exec("print(string.rep('⚛️', 200000))", "probe.lua").unwrap();
        let log = sandbox.take_log();
        assert!(log[0].len() <= LOG_LIMIT && log[0].chars().all(|c| matches!(c, '\u{269b}' | '\u{fe0f}')));
    }

    #[test]
    fn bytecode_is_refused() {
        let sandbox = Sandbox::new().unwrap();
        let bytecode = sandbox.lua.load("return 1").into_function().unwrap().dump(true);
        let error = sandbox.lua.load(&bytecode[..]).set_mode(ChunkMode::Text).exec().unwrap_err().to_string();
        assert!(error.contains("binary chunk"), "{error}");
    }

    #[test]
    fn finalisers_are_refused() {
        let sandbox = Sandbox::new().unwrap();
        let message = error(&sandbox, "local t = setmetatable({}, {__gc = function() end})");
        assert!(message.contains("probe.lua:1: plugins cannot use __gc metamethods"), "{message}");
        let value = sandbox.exec("return getmetatable(setmetatable({}, {__index = {a = 1}})).__index.a", "probe.lua");
        assert_eq!(value.unwrap().as_integer(), Some(1));
    }

    #[test]
    fn tostring_may_print() {
        let sandbox = Sandbox::new().unwrap();
        sandbox
            .exec(
                "local t = setmetatable({}, {__tostring = function() print('inner') return 'outer' end}) print(t)",
                "probe.lua",
            )
            .unwrap();
        assert_eq!(sandbox.take_log(), ["inner", "outer"]);
    }

    #[test]
    fn many_large_log_arguments_keep_only_the_tail() {
        let sandbox = Sandbox::new().unwrap();
        sandbox
            .exec(
                "local s = string.rep('x', 8 * 1024 * 1024) local t = {} for i = 1, 200 do t[i] = s end \
                 t[201] = 'end' print(table.unpack(t))",
                "probe.lua",
            )
            .unwrap();
        let log = sandbox.take_log();
        assert_eq!(log.len(), 1);
        assert!(log[0].len() <= LOG_LIMIT && log[0].ends_with("x\tend"), "{}", log[0].len());
    }

    #[test]
    fn the_watchdog_stops_a_call_that_runs_too_long() {
        let fired = Arc::new(Mutex::new(None));
        let record = fired.clone();
        let action: TimeoutAction = Arc::new(move |label: &str| *record.lock().unwrap() = Some(label.to_string()));
        let sandbox = Sandbox::with_watchdog(Duration::from_millis(200), action).unwrap();
        std::thread::spawn(move || {
            let _ = sandbox.exec(
                "return string.find(string.rep('a', 26), string.rep('a?', 26) .. string.rep('a', 26) .. 'b')",
                "slow.lua",
            );
        });
        let start = Instant::now();
        while fired.lock().unwrap().is_none() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(fired.lock().unwrap().as_deref(), Some("slow.lua"));
    }
}
