# The bundled plugin

The janky repository has one first-party plugin: `plugins/elevenlabs.lua`. It is a media generator
that makes text-to-speech audio with the [ElevenLabs](https://elevenlabs.io) service.

The plugin is not part of the `janky` program. It is a script that you copy into your own project.

> [!WARNING]
> ElevenLabs is a paid service. Each job uses credits from your ElevenLabs account. A change to any
> variable makes a new job for every note, and so costs the full amount again. See
> [What makes a job new](generating.md#what-makes-a-job-new).

## Set up the instance

1. Copy `elevenlabs.lua` into the `plugins/` directory of your project.
2. Add the instance to `deck.toml`:

   ```toml
   [plugins.voice]
   path = "plugins/elevenlabs.lua"
   [plugins.voice.http]
   allowed_hosts = ["api.elevenlabs.io"]
   max_concurrency = 2
   [plugins.voice.secrets]
   api_key = { env = "ELEVENLABS_API_KEY" }
   [plugins.voice.variables]
   voice_id = "JBFqnCBsd6RMkjVDRZzb"
   model = "eleven_v4"
   output_format = "mp3_44100_128"
   ```

3. Put your ElevenLabs API key in `.env` (see [Secrets](instances.md#secrets)).
4. Call the instance from a field template:

   ```jinja
   {{ entry.name | nfc | media('voice') | sound }}
   ```

The plugin sends the text that the template gives it. In this example, that is the `name` of the
entry.

## Variables

The plugin has no default values. You must set `voice_id`, `model` and `output_format`.

| Variable | Type | Required | Meaning |
|---|---|---|---|
| `voice_id` | text | yes | The id of the ElevenLabs voice. Letters and digits only. The example uses "George", a stock British English voice. |
| `model` | text | yes | The ElevenLabs model id. The example uses `eleven_v4`. Check the model names in your ElevenLabs account. |
| `output_format` | one of a list | yes | The audio format (see below). |
| `language` | text | no | A language code for ElevenLabs, for example `en`. If you do not set it, the plugin does not send one. |
| `voice_settings` | table | no | `stability` and `similarity_boost` (both numbers). No other keys are allowed. |
| `experimental_trailing_silence` | number | no | Seconds of silence to add at the end of each clip, more than 0 and at most 3. It works only with an `mp3_*` format. |

The output formats are:

| `output_format` | File type |
|---|---|
| `opus_48000_32`, `opus_48000_64`, `opus_48000_96`, `opus_48000_128`, `opus_48000_192` | Ogg Opus (`.ogg`) |
| `mp3_44100_64`, `mp3_44100_96`, `mp3_44100_128`, `mp3_44100_192` | MP3 (`.mp3`) |

Use an MP3 format if your cards use Anki's `[sound:...]` tag (the `sound` filter). On Anki mobile,
Ogg Opus plays only through an HTML `<audio>` element, so autoplay does not work with it.

## Cost and errors

- `janky plan` counts the characters of the pending jobs, for example `Generator jobs: 9 to run (13
  characters)`. This count is not the price.
- After a run, janky prints `Reported cost for voice: <n>`. This number comes from ElevenLabs, and it
  counts credits, not characters.
- If ElevenLabs answers with an error, the job is refused, with the message
  `ElevenLabs HTTP <status>: <detail>`. janky runs a refused job again on the next run.
- A wrong API key or an empty quota comes back as HTTP 401. janky then stops the run (see
  [Unauthorised stops the run](generating.md#unauthorised-stops-the-run)).
