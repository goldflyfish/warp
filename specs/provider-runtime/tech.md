> Historical implementation notes. For current MyWarp 0.3.0 behavior and native provider setup, see [the setup guide](provider-setup.md).

# Provider runtime integration

## Existing boundaries

The public repository contains the Rust client. Its built-in agent request path is
`app/src/ai/agent/api/impl.rs::generate_multi_agent_output`: it creates a multi-agent request
with API keys and custom model-provider settings and passes it to `warp_multi_agent_client`.
Changing the custom endpoint list alone does not replace this agent loop.

External runtimes are selected in `app/src/ai/agent_sdk/driver/harness/mod.rs`. ACP implementations
live in the adjacent `acp/` directory. They map session events to the client conversation model
and route prompts and cancellations through
`app/src/ai/blocklist/controller/external_harness.rs`.

The existing `Harness` enum in `crates/warp_cli/src/agent.rs` already includes OpenCode. Upstream
allows it for local child-agent terminal launches but rejects it for regular agent runs. The
initial patch fills this integration gap.

## Initial patch

- Route OpenCode through ACP regardless of the generic transport default, as Pi already does.
- Mark OpenCode conversations as native on both transport values so queue/cancellation routing
  follows the effective runtime.
- Resolve the built-in launch command to `opencode acp` without requiring a sidecar manifest.
- Identify the runtime with `CLIAgent::OpenCode` and provide installation documentation.
- Remove the regular-run rejection of OpenCode.
- Reuse ACP model discovery and `session/set_config_option`; a requested model absent from the
  runtime's advertised choices fails explicitly.
- Leave OpenCode provider configuration and credentials in OpenCode's own supported store.

OpenCode 1.19.4 was installed in the task's temporary tools directory for compatibility testing.
An isolated credential-free process completed ACP v1 `initialize` and `session/new` and returned
a `model` select setting. No paid model turn was sent by that handshake check.

A second real OpenCode process selected a custom model through `session/set_config_option`,
called a local OpenAI-compatible HTTP fixture with a dummy key, streamed the fixture's response
through `session/update`, and completed with `end_turn`. This verifies protocol compatibility;
it is not a live OpenAI, Anthropic, Z.ai, or native Warp GUI test.

## Try after building the fork

Install OpenCode according to its official instructions and connect a provider from its normal
interactive UI with `/connect`, then choose a model with `/models`. Use provider-supported
authentication; Claude subscription credentials must not be reused in OpenCode.

With this fork's CLI on PATH:

```text
oz agent run --harness opencode --prompt "Explain this repository without changing files"
```

To select a model, use a provider/model ID returned by OpenCode. Model IDs and access must be
discovered from the connected runtime rather than hard-coded in Warp.

This initial driver still follows Warp's task/conversation service setup. It is not a standalone
offline agent service. It also inherits the ACP driver's unattended approval semantics, so it
must not be presented as the final attended desktop replacement.

## Validation

The added Rust coverage checks that OpenCode resolves to a native ACP harness with the default
transport and that manifest discovery selects `opencode acp` without a sidecar. Existing ACP
mapping, model-selection, permission, and cancellation tests remain relevant.

`cargo test --locked -p warp --lib opencode_ -- --nocapture` passed: 12 tests, 0 failures.
The test build used the repository's Rust 1.92.0 toolchain on Windows, with Protocol Buffers
36.2 supplied as a local build tool.

`cargo clippy --locked -p warp -p warp_cli --all-targets --tests -- -D warnings` passed.
Final formatting passed for both modified packages using the repository's import-format
configuration, followed by `git diff --check`. Tests and lint were not rerun after formatting,
as specified by `AGENTS.md`.

## Desktop local runtime launcher

`LocalAgentRuntime` in `app/src/settings/ai.rs` identifies the three supported local CLI
runtimes. `AISettings.local_agent_runtime` persists the selection at
`agents.third_party.local_agent_runtime`, with `SyncToCloud::Never`. The default is OpenCode.
This preference applies to **New local agent tab**, not the existing Warp Agent tab action.

`CLIAgentsPageView` exposes a dropdown, a launch action, and runtime-specific setup guidance.
Runtime and provider-connection rows are separate searchable widgets, with a page-level title.
Local-launch widgets are only constructed for builds with `local_tty`.

`WorkspaceAction::AddLocalAgentTab` is registered in the command palette and new-session menu.
It creates a new terminal with `DefaultSessionModeBehavior::Ignore` and uses the existing
pending-command queue, so the bare `opencode`, `codex`, or `claude` command is submitted after
shell bootstrap. No prompts, credentials, interpolated arguments, or permission-bypass switches
are added. Missing CLIs fail visibly in the terminal; the action does not fall back to Warp.

The desktop launcher uses each CLI's own attended permission UI. It does not route through
the unattended ACP driver. The separate native desktop bridge below uses attended ACP approvals.

Desktop-launcher validation on Windows:

- `cargo test --locked -p warp --lib local_agent_ -- --nocapture`: 54 passed, 0 failed.
- `cargo test --locked -p warp --lib test_unified_new_session_menu`: 2 passed, 0 failed.
- `cargo clippy --locked -p warp --all-targets --tests -- -D warnings`: passed.

The new tests cover runtime-dependent pending commands in fresh tabs, preservation of existing
input, launcher availability with Warp AI disabled, a machine-local preference, and settings
search isolation with its page title retained. They use WarpUI's in-process test harness and
mocked terminal resources. A live desktop shell launch, visual layout, and authenticated
provider sessions still require end-to-end verification.

## Native desktop bridge

`controller/provider_chat.rs` routes provider-bound user prompts before Warp slash commands,
request construction, and inference. `acp/desktop.rs` manages a local `opencode acp` process and
reuses the ACP connection, event mapper, and native external-harness response pipeline.
`agent_input_footer/provider_controls.rs` supplies runtime/model dropdowns, status, errors,
retry, cancellation, and approval controls. Native runtime preference is machine-local and is
independent of the interactive CLI launcher's preference.

`ProviderChatSession` persists a binding ID, runtime, absolute working folder, session ID, and
provider/model ID in local conversation data. Restored chats remain external-harness chats;
unknown runtimes fail explicitly. The worker calls `session/load` for saved sessions, checks
`loadSession` support, and requires exact `session/set_config_option` model confirmation.
Runtime changes are limited to empty chats; provider session forks are rejected. Synthetic
local stream IDs are not saved as Warp server conversation/task IDs. Passive suggestion request
construction rejects provider conversations.

The child inherits provider configuration and receives a process-only
`OPENCODE_CONFIG_CONTENT` permission override of `ask`; global configuration files are untouched.
Filesystem/terminal client capabilities are false, so OpenCode runs its own tools. ACP permission
requests wait for an exact UI decision. Only `allow_once` is accepted, with explicit Warp command
denylist/protected-path refusals enforced first. Pending approvals are dropped on stop/disconnect;
requests and streamed updates are restricted to the bound session. Windows uses an owned job
object to terminate the runtime and its non-breakaway children when the worker is dropped.

Setup, model selection, and stopping have bounded waits. A model confirmation or prompt error
closes the connection; it never retries a prompt against a different provider. Missing saved
models leave prompts blocked while the user explicitly chooses an available model. Plain text
and local host folders are supported initially; attached context, SSH/WSL execution, shared/cloud
sessions, queued follow-ups, and provider forks require additional integration.

Native regression coverage exercises saved-session loading, unavailable model/runtime behavior,
exact model confirmation, attended approvals and cancellation, persistence, busy-query rejection,
and streamed output through the real native history pipeline using an in-memory ACP peer.
These tests do not replace a live desktop or authenticated-provider run.

## Native validation

Windows / Rust 1.92.0:

- `cargo test --locked -p warp --lib provider_chat -- --nocapture`: 15 passed, 0 failed.
- `cargo test --locked -p command scoped_job_ -- --nocapture`: 1 passed, 0 failed.
- `cargo clippy --locked -p warp -p command -p persistence --all-targets --tests -- -D warnings`:
  passed. Final formatting follows the repository validation order.
- Real OpenCode 1.19.4 against an isolated local HTTP fixture: exact model selection,
  attended write approval, denied write, and saved-session loading all passed. The fixture
  used a dummy key and temporary files, without personal or project content.
- A requested OpenRouter live turn failed with `User not found`. OpenRouter's direct
  authentication endpoint independently returned HTTP 401 with the same message. After the
  user saved a replacement key through OpenCode's file-reference config, authentication returned
  HTTP 200 and a real `openrouter/openai/gpt-4.1-nano` protocol turn streamed
  `MYWARP_OPENROUTER_OK` and completed with `end_turn`. No project content was sent.

The automated native tests use WarpUI's in-process app harness and an in-memory ACP peer.
Live desktop layout and a provider turn through the native desktop window remain unverified.
The authenticated OpenRouter protocol turn passed separately.

## Windows package profiles

The root `build-portable.bat` / `.py` and `build-installer.bat` / `.py` entry points share
`script/windows/package_mywarp.py`. They build the OSS binary with `release_bundle,gui`,
stage the Windows runtime DLLs, PowerShell bootstrap, bundled resources, license notices and
a native OpenCode executable, then create a portable ZIP or compile a per-user Inno installer.
Existing outputs are refused rather than replaced; build-machine profiles and credentials are
never packaging inputs. See [Windows packaging](windows-packaging.md) for flags and prerequisites.

`warp_core::windows_package` detects `mywarp-portable` or `mywarp-installed` beside the running
executable. Portable profiles live in the adjacent `Configurations` directory. Installed profiles live
in the Windows known Local AppData folder under `MyWarp/Configurations`. Config, data, state and cache helpers,
including home-style Warp config, use that profile. Executables without a marker retain their
existing paths. Single-instance mutexes and URI pipes use distinct installed/portable namespaces.

Package initialization migrates legacy profiles before opening configuration files. A nonempty
`Configurations` folder takes precedence over legacy data. Interrupted migrations resume using a
marker and never overwrite conflicting files. The OpenCode shim invokes `MyWarp.exe --prepare-profile`
so launching the runtime directly uses the same initialization and migration as the GUI.

The OSS entry point initializes a fresh profile before starting threads, and adds the bundled
OpenCode shim to its process PATH. Native chats resolve the bundled native executable directly.
Both native and terminal OpenCode launches use profile-scoped XDG/config directories; inherited
inline OpenCode config is discarded for packaged launches. The native worker still forces
attended `permission: ask`. Existing provider config is preserved, and provider API-key
environment variables remain supported. The installer registers per-user shortcuts and an
uninstaller while preserving the separate user profile on uninstall.

## Remaining service boundary

A local-only session implementation would additionally need to replace server-owned task IDs,
conversation setup/persistence, and shared-session assumptions. Keep cloud sharing optional in
that implementation, and verify that no inference credentials or local transcript are sent to
Warp services unless the user has selected a feature requiring that transmission.
