# Native provider and agent setup (MyWarp 0.3.0)

MyWarp's Windows Settings page configures providers directly. It does not require an application account or send requests to Warp services.

## Connect a provider

Open **Settings → Agents → AI providers**. Select a provider from the left-hand list, or use **+ Add provider** to search the complete directory. The right-hand panel keeps the connection details and model list together. For an API connection, paste the key, optionally change the base URL, and save. An empty key field keeps an already saved key; it never displays that key. To replace a key, paste the replacement and save again.

**Z.AI Coding Plan** and **Z.AI Standard API** are separate entries. Coding Plan uses `https://api.z.ai/api/coding/paas/v4`; Standard API uses `https://api.z.ai/api/paas/v4`. The form displays the effective endpoint and explains which account balance each entry uses.

Built-in providers use their standard endpoints unless overridden. A custom OpenAI-compatible provider needs a base URL and model ID. Remote providers require HTTPS; local model servers can use HTTP on loopback.

Provider discovery does not imply that a connection was configured. **No credentials**, **Available · Not tested**, or **Key available · Not tested** describe the current state. The form identifies a key in the provider configuration, an API key or sign-in in the profile auth file, or a provider merely detected by the runtime. Imported credentials are identified by their source; they do not imply manual setup or a working connection. The default provider has a green marker; other providers with a successfully tested model have yellow markers. **Verified** requires a successful test for the selected model against its current connection configuration. Successful checks are saved in the profile and restored after closing Settings, refreshing, or restarting the application. Changing that provider's credentials, endpoint or tested model configuration requires a new test; changing the default model, another provider or agent settings preserves verification. Pick a model and select **Test connection** to send a short verification prompt. The check uses an isolated local session with every tool denied. It does not use your terminal's project files. A successful response is shown as **Connection verified**; rejected credentials, unavailable models and provider errors are shown in the page.

While testing, the model panel shows elapsed time and **Cancel test**. Checks stop after 30 seconds and stop automatic provider retries on the first failure. Billing errors such as Z.AI code 1113 are identified separately from ordinary rate limits. Save a changed API key or URL before testing it.

Earlier builds kept verification only in memory. Test previously configured models once after upgrading to save their verification; existing keys, endpoints and model defaults remain configured.

**Set as default** saves the selected provider/model as the default for new chats and confirms the saved model. Existing conversations retain their selected model and history. MyWarp checks provider configuration files every five seconds and reloads external edits automatically. An idle conversation reconnects with its existing session and selected model; an active response finishes before applying the change. **Reconnect** remains available for a manual retry.

The chat footer contains a **Profile** selector, the conversation's selected model, and a **Reasoning** selector. Choose saved model defaults in **Profiles**, or configure provider defaults in **Providers**. The **Reasoning** selector shows the levels supported by the current model, including provider-specific options. Changes apply while the conversation is idle and are saved with the conversation for reconnects and restores. Models without configurable reasoning show **Reasoning: Not supported**.

## Sign-ins and local CLI agents

Provider sign-in choices come from the bundled runtime. OpenAI's supported browser and headless sign-ins open the provider's authorization page, then complete from the native settings form. Code-based methods have a masked authorization field. Cancel terminates the pending local sign-in flow.

Anthropic API keys connect native chat. The bundled runtime does not offer a native Claude subscription OAuth method; use the separately installed Claude Code CLI and its supported login from **Third party CLI agents**. Codex and other installed CLI agents also retain their own supported login/configuration paths. Credentials are not silently copied between unrelated CLI products.

## Warp Agent behavior

Open **Settings → Agents → Warp Agent behavior** for the original row-based Warp Agent controls. The page retains Active AI, Input, Voice and display settings, independently of provider configuration. Search filters individual controls; clearing the query restores the full page.

The master switch enables or disables MyWarp's native agent without requiring a Warp account. It uses a new local preference so older account-onboarding choices cannot disable a newly configured provider. Existing input and display preferences retain their stored values.

Active AI uses the default provider/model selected in **AI providers** for Next Command, prompt and code suggestions, block titles, and commit/PR generation. Command search, query completion, assistant dialogue and workflow metadata use the same provider path. Background requests run in isolated sessions with every tool denied, stop retries on the first provider failure, and have a 30-second generation deadline. Changing provider credentials or the default model reloads the background runtime. These requests use the provider's own quota and billing.

Code suggestions use only the context files allowed by the existing file permissions. Returned edits must name a supplied file and uniquely match original text. They appear in the existing review UI; background generation does not write those files. Turning a feature off or changing the terminal context suppresses stale results.

Voice input uses offline Windows speech recognition. Automatic language selection uses an installed recognizer; an explicit language requires its Windows speech language pack. No recording is sent to Warp or the chat provider. Local speech is not subject to Warp voice quotas.

Block export creates an HTML file in the profile's `shared-blocks` directory and copies its local file link. It does not publish a public web URL or require a Warp account. Block titles are generated by the configured AI provider. Provider configuration, native agent profiles, enforced tool permissions, personal instructions and MCP configuration remain in **AI providers**.

## Execution profiles

Open **Settings → Agents → Profiles** or select **Profiles** in the chat footer. The original profile cards and editor let you create, rename and remove profiles, save a provider model, and configure file reading, code changes, command execution and allow/denylists. Profiles remain in the existing file-backed execution-profile collection, and work without a Warp account. Existing profile data is retained.

Profile model pickers use the configured providers' connected models, including custom endpoints, without Warp credit or upgrade restrictions. New conversations use the active profile's model; an unset model follows the provider default. Selecting a different profile in chat applies its model and permissions. Editing its model updates an idle chat, or waits for the current turn to finish. Changing a profile's name or permissions preserves any conversation-specific model selection. An unavailable saved model is reported and blocks sending until a valid model is chosen; it never silently falls back.

File and command approvals use the active execution profile. Explicit command denylists, protected paths and runtime-denied tools remain enforced. Unknown tools still require attended approval. Provider modes, personal instructions and MCP setup remain available in **AI providers**.

## Agent modes, tools and instructions

Select Build, Plan, or a saved primary agent profile. New custom profiles have a name and role/instructions. Select a saved profile to edit or remove it. Removing the active custom profile restores Build.

Tool permission policies apply to the native agent:

- **Ask before every tool** requests attended approval for tools.
- **Allow reading; ask before changes** allows read/search/list operations and asks for other tools.
- **Read only** allows reading/search and denies commands, edits and other tools.

Approval requests appear in the chat footer with allow-once and deny controls. Invalid saved permission preferences prevent the runtime from connecting rather than silently broadening access.

Personal instructions are saved in the application's profile and added to existing configured instruction sources. Project-specific OpenCode configuration and repository instruction files continue to follow the runtime's precedence rules.

## MCP servers

Add a remote server URL or select a local command. For a local server, enter the executable separately from its arguments, with one argument per line. Arguments are passed directly without shell parsing. Existing servers have Edit, Enable/Disable and Remove controls. Removing entries preserves unrelated JSONC configuration and comments and reloads the settings runtime so deleted entries disappear.

## Files and profile locations

Portable applications read configuration files from `Configurations` beside the executable. Installed applications read `%LOCALAPPDATA%\MyWarp\Configurations`. Copy only this directory to transfer settings, themes, providers and profiles; no runtime database is required. Close MyWarp and OpenCode first. Generated files and provider sign-in sessions are stored separately in `Runtime`. Older combined profiles are split automatically and existing copied configuration is preserved.

Relative to `Configurations`:

| File or folder | Purpose |
| --- | --- |
| `settings.toml` | Appearance, shortcuts and native execution profiles |
| `themes/` | Custom themes and their assets |
| `providers/opencode.jsonc` | Provider/model defaults, provider agent profiles and MCP configuration |
| `providers/keys/` | Separate API-key files referenced by configuration |
| `providers/verification.json` | Successful provider/model checks, stored as connection fingerprints without credentials |
| `agent-preferences.json` | Native agent mode and enforced tool policy |
| `agent-instructions.md` | Personal instructions entered in Settings |

Provider sign-ins are in `Runtime/providers/data/opencode/auth.json`; startup diagnostics are in `Runtime/logs/provider-runtime.log`. Runtime plugin dependencies and generated package files are also kept outside `Configurations`. Native provider edits update the canonical JSONC file while preserving comments; runtimes reload that file after changes. Personal instruction references use a runtime-provided configuration path variable rather than a machine-specific absolute path.

First launch imports an existing OpenCode configuration into a fresh profile, including referenced key files and supported provider sign-ins. Existing application-specific configuration is retained. A failure to import an old configuration does not prevent opening the native form to repair the connection.

Runtime change detection includes `opencode.jsonc`, referenced `{file:...}` values, key files, `auth.json`, personal instructions and agent preferences. Reloading updates the open Settings page and existing native chats. Unsaved key, endpoint, model and instruction edits in the form are preserved. Invalid JSONC, malformed credential/preferences JSON, invalid permission preferences and missing referenced files show a diagnostic while the existing chat runtime remains active. Correcting the file triggers recovery on the next check. Background requests also check the same revision before selecting their runtime.

Windows provider processes inherit their parent job restrictions; they do not require breakaway permission. Each runtime has an owned job for child-process cleanup.

## Validation and rendering

`provider_setup_tests.rs` contains pure configuration tests and explicit real-runtime tests for saving, responding, credential rotation/rejection, disconnect, custom profiles and MCP removal. The optional OpenRouter check uses a separately stored key and the same backend as the native form.

`provider-settings-check` is a manual Windows renderer check. Build it with `cargo build -p warp --bin provider-settings-check --features release_bundle,gui,provider-settings-check`; the dedicated feature enables native frame capture. Run it from a disposable portable staging folder with generic application resources and a fresh profile. It uses the application's integration driver to capture actual native frames and can exercise the form against a local HTTP fixture. The general `crates/integration` runner currently lacks Windows hermetic startup support, so this dedicated check keeps its configuration inside the staging folder. The watch check edits models and a referenced synthetic key without Refresh/Reconnect, verifies that existing conversation history and the selected model survive, and checks malformed-file recovery and preservation of unsaved form edits. The layout check clicks the master switch, Active AI switch and every restored AI/voice feature switch in both directions. The auxiliary check sends all ten background text features to a local HTTP fixture, rejects exposed tools, checks filtering of unknown file paths, and verifies local block export/removal. Local speech has a separate manual test using a synthesized WAV recording.

The runtime's [server API](https://opencode.ai/docs/server/) and [configuration reference](https://opencode.ai/docs/config/) describe the underlying provider capabilities. Account and Warp service blocking remains enforced by MyWarp itself.

Saved API keys display a fixed `********` mask. Typing replaces the key; an untouched masked field preserves the saved credential. **Open provider config** opens the profile's `opencode.jsonc` in MyWarp's built-in editor for provider options and models. **Open key file** opens a referenced key file, the configuration for an inline key, or the imported API-auth file. The configuration button remains available when the provider runtime is loading or needs repair.
