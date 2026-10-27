# MyWarp

An independent terminal with a local, configurable AI agent. MyWarp starts in the terminal without a MyWarp or Warp account. Provider connections use your own credentials; Warp's account, signup, billing, telemetry and hosted services are disabled in the MyWarp executable.

## Set up your AI

Open **Settings → Agents → AI providers**.

1. Choose OpenAI, Anthropic, OpenRouter, Z.ai, another provider from the searchable directory, or a custom OpenAI-compatible endpoint.
2. Paste an API key and select **Save connection**, or use a sign-in method offered by that provider.
3. Select a model and choose **Test connection**. A saved credential is marked configured; a successful model response verifies the connection.
4. Choose **Use for new chats** to set the default model.

The chat footer provides model selection, provider settings, reconnect, stop and tool approval controls. Reconnect an idle existing conversation after changing credentials or agent settings.

Supported subscription sign-ins are listed by the bundled runtime. OpenAI offers its supported browser and headless flows. Claude API keys work in native chat; Claude Code's own subscription login is available through the separate local CLI launcher under **Third party CLI agents**.

## Agent settings

The same page includes Build, Plan and custom agent profiles; tool permission policies; personal instructions; and local or remote MCP servers. Profiles can be created, edited and removed. MCP servers can be edited, enabled, disabled and removed. Local command arguments have their own field, one argument per line.

Native chat currently uses OpenCode over ACP. It supports local host folders, streaming responses, conversation persistence and attended tool approvals. Attachments, prompt queues, conversation forks, remote SSH/WSL contexts and Warp's hosted passive AI features do not have equivalent native implementations yet.

## Windows packages

Run either root script to build the application and create its package:

```bat
build-portable.bat
build-installer.bat
```

The default package version is **0.3.0**. Outputs go to `Packages\Portable` and `Packages\Installer`; build logs and HTML reports go to `Packages\Logs`.

| Distribution | Configuration and local state |
| --- | --- |
| Portable | `profile\` beside `MyWarp.exe` |
| Installed | `%LOCALAPPDATA%\MyWarp\` |

The installed executable goes under `%LOCALAPPDATA%\Programs\MyWarp`. Distributed packages contain fresh profiles without credentials from the build computer. Existing OpenCode configuration and sign-ins are imported into a fresh application profile on first GUI launch.

API keys entered in Settings are stored as separate local files, referenced by the provider configuration. See the [provider setup guide](specs/provider-runtime/provider-setup.md) and [build instructions](specs/provider-runtime/windows-packaging.md).

## Development and attribution

This is a fork of [warpdotdev/warp](https://github.com/warpdotdev/warp). The terminal and native UI use the upstream Rust/WarpUI code; the MyWarp provider bridge is developed in this fork. See [AGENTS.md](AGENTS.md) for repository development and validation instructions.

Original source licenses and notices remain in [LICENSE-AGPL](LICENSE-AGPL) and [LICENSE-MIT](LICENSE-MIT). Windows distributions include dependency notices and the bundled runtime's license.
