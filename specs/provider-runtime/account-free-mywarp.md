# Account-free MyWarp (0.2.0)

The MyWarp OSS executable starts directly in the terminal. It neither creates a Warp account nor loads an old Warp identity from secure storage. Account, signup, billing, referrals, teams, Warp Drive and hosted-cloud settings are absent from its settings navigation and search. Attempts to open old account settings land on AI providers instead.

## AI

MyWarp AI uses OpenCode ACP and the provider/model selected in the native chat footer. Warp inference is unavailable, including when old preferences selected Warp. Existing local OpenCode provider configuration and provider sign-ins are imported into an empty MyWarp profile on first use. Referenced credential files are copied into that profile and file references rewritten; existing customized profile configuration and existing profile sign-ins are preserved. Imported credentials are local user data and are never included in build artifacts.

Use Settings > Agents > AI providers to open provider setup or the configuration file. Provider setup opens a local OpenCode tab; use its /connect flow. Codex and Claude Code can also run as separately installed local agents using their supported logins. Native chat currently uses the OpenCode adapter; native dedicated Codex/Claude adapters are not implemented.

Portable profile: profile beside MyWarp.exe. Installed profile: %LOCALAPPDATA%\MyWarp. Provider configuration: providers/config/opencode/opencode.jsonc inside that profile. API keys can use {file:./keys/your-provider.key} or the runtime's supported environment variables.

## Service policy

The OSS channel has no production server, telemetry, crash reporting, updater or static hosted MCP configuration. Its placeholder HTTP/WebSocket endpoints use loopback port zero. Shared HTTP clients reject Warp domains and Warp Firebase authentication origins before transport hooks or network I/O, reject redirected requests to those origins, and terminate SSE requests locally. Provider APIs and local model endpoints remain allowed. The app's URL opening hook blocks Warp service links; account, upgrade, cloud and sharing navigation is removed or disabled.

Warp-hosted passive suggestions, voice transcription, sharing and cloud execution are disabled. Ordinary native provider turns stream text/tool activity into local conversation history and retain attended tool approvals. Attachments, queued follow-ups, forks and remote/WSL native sessions remain unsupported and report an error instead of falling back to Warp.

## Packages

Both root build scripts build the real release executable, bundle OpenCode and write packages under Packages/Portable and Packages/Installer. The default version is 0.2.0 to distinguish this behavior from older 0.1.0 builds. Distributed profiles are fresh and contain no credentials from the build computer. First GUI launch imports existing local provider configuration; schema-generation smoke tests intentionally do not import credentials.
