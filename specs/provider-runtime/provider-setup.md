# Native provider and agent setup (MyWarp 0.3.0)

MyWarp's Windows Settings page configures providers directly. It does not require an application account or send requests to Warp services.

## Connect a provider

Open **Settings → Agents → AI providers**. Use the quick choices or search the complete provider directory. For an API connection, paste the key, optionally change the base URL, and save. An empty key field keeps an already saved key; it never displays that key. To replace a key, paste the replacement and save again.

Built-in providers use their standard endpoints unless overridden. A custom OpenAI-compatible provider needs a base URL and model ID. Remote providers require HTTPS; local model servers can use HTTP on loopback.

The page shows **Configured** when the runtime has credentials. This is separate from a successful connection test. Pick a model and select **Test connection** to send a short verification prompt. The check uses an isolated local session with every tool denied. It does not use your terminal's project files. A successful response is shown as **Connection verified**; rejected credentials, unavailable models and provider errors are shown in the page.

**Use for new chats** saves the provider/model default. Existing conversations retain their selected model and history. Use the chat footer's **Reconnect** to apply changed credentials, instructions or agent settings to an idle conversation.

## Sign-ins and local CLI agents

Provider sign-in choices come from the bundled runtime. OpenAI's supported browser and headless sign-ins open the provider's authorization page, then complete from the native settings form. Code-based methods have a masked authorization field. Cancel terminates the pending local sign-in flow.

Anthropic API keys connect native chat. The bundled runtime does not offer a native Claude subscription OAuth method; use the separately installed Claude Code CLI and its supported login from **Third party CLI agents**. Codex and other installed CLI agents also retain their own supported login/configuration paths. Credentials are not silently copied between unrelated CLI products.

## Profiles, tools and instructions

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
| `agent-preferences.json` | Native agent mode and enforced tool policy |
| `agent-instructions.md` | Personal instructions entered in Settings |

Provider sign-ins are in `Runtime/providers/data/opencode/auth.json`; startup diagnostics are in `Runtime/logs/provider-runtime.log`. Runtime plugin dependencies and generated package files are also kept outside `Configurations`. Native provider edits update the canonical JSONC file while preserving comments; runtimes reload that file after changes. Personal instruction references use a runtime-provided configuration path variable rather than a machine-specific absolute path.

First launch imports an existing OpenCode configuration into a fresh profile, including referenced key files and supported provider sign-ins. Existing application-specific configuration is retained. A failure to import an old configuration does not prevent opening the native form to repair the connection.

Windows provider processes inherit their parent job restrictions; they do not require breakaway permission. Each runtime has an owned job for child-process cleanup.

## Validation and rendering

`provider_setup_tests.rs` contains pure configuration tests and explicit real-runtime tests for saving, responding, credential rotation/rejection, disconnect, custom profiles and MCP removal. The optional OpenRouter check uses a separately stored key and the same backend as the native form.

`provider-settings-check` is a manual Windows renderer check. Build it with `cargo build -p warp --bin provider-settings-check --features release_bundle,gui,provider-settings-check`; the dedicated feature enables native frame capture. Run it from a disposable portable staging folder with generic application resources and a fresh profile. It uses the application's integration driver to capture actual native frames and can exercise the form against a local HTTP fixture. The general `crates/integration` runner currently lacks Windows hermetic startup support, so this dedicated check keeps its configuration inside the staging folder.

The runtime's [server API](https://opencode.ai/docs/server/) and [configuration reference](https://opencode.ai/docs/config/) describe the underlying provider capabilities. Account and Warp service blocking remains enforced by MyWarp itself.
