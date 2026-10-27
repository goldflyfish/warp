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

Portable applications use the `Configurations` directory beside the executable. Installed applications use `%LOCALAPPDATA%\MyWarp\Configurations`. The executable and provider runtime use the same profile. Close both before copying this entire folder to transfer settings and provider connections. Legacy profiles migrate automatically when `Configurations` is absent or empty; existing copied configurations are preserved.

Relative to that profile:

| File or folder | Purpose |
| --- | --- |
| `providers/config/opencode/opencode.jsonc` | Provider/model defaults, profiles and MCP configuration |
| `providers/config/opencode/keys/` | Separate API-key files referenced by configuration |
| `providers/data/opencode/auth.json` | Provider sign-ins, when present |
| `agent-preferences.json` | Native agent mode and enforced tool policy |
| `agent-instructions.md` | Personal instructions entered in Settings |
| `logs/provider-runtime.log` | Settings runtime startup diagnostics |

First launch imports an existing OpenCode configuration into a fresh profile, including referenced key files and supported provider sign-ins. Existing application-specific configuration is retained. A failure to import an old configuration does not prevent opening the native form to repair the connection.

Windows provider processes inherit their parent job restrictions; they do not require breakaway permission. Each runtime has an owned job for child-process cleanup.

## Validation and rendering

`provider_setup_tests.rs` contains pure configuration tests and explicit real-runtime tests for saving, responding, credential rotation/rejection, disconnect, custom profiles and MCP removal. The optional OpenRouter check uses a separately stored key and the same backend as the native form.

`provider-settings-check` is a manual Windows renderer check. Build it with `cargo build -p warp --bin provider-settings-check --features release_bundle,gui,provider-settings-check`; the dedicated feature enables native frame capture. Run it from a disposable portable staging folder with generic application resources and a fresh profile. It uses the application's integration driver to capture actual native frames and can exercise the form against a local HTTP fixture. The general `crates/integration` runner currently lacks Windows hermetic startup support, so this dedicated check keeps its configuration inside the staging folder.

The runtime's [server API](https://opencode.ai/docs/server/) and [configuration reference](https://opencode.ai/docs/config/) describe the underlying provider capabilities. Account and Warp service blocking remains enforced by MyWarp itself.
