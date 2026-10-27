> Historical implementation notes. For current MyWarp 0.3.0 behavior and native provider setup, see [the setup guide](provider-setup.md).

# Multi-provider Warp fork

## Goal

Keep Warp's terminal and conversation UI while allowing the user to choose the agent runtime,
model provider, and authentication method. Initial targets are Codex, Claude, and Z.ai, with
both existing agent logins and API keys where supported by the provider.

The project fork is https://github.com/goldflyfish/warp. Its initial base is upstream commit
`1e6c3ae465d6a925a9b97ae03877aaadd159b8f1`.

## Product model

Agent runtime, inference provider, and authentication are separate choices:

| Runtime | Provider | Authentication |
| --- | --- | --- |
| Codex | OpenAI or a runtime-supported endpoint | Existing Codex login or API key |
| Claude Code terminal | Anthropic | The user's own CLI login or API key |
| OpenCode | OpenAI | Supported ChatGPT login or API key |
| OpenCode | Anthropic | API key |
| OpenCode | Z.ai | API key; use the endpoint appropriate to the user's plan |
| OpenCode | Other supported or custom providers | Provider-supported credentials |

Codex is an agent runtime; OpenAI is its model provider. The same distinction applies to
Claude Code and Anthropic. A provider switch must not silently replace the user's agent runtime.

Anthropic's Agent SDK documentation says third-party products cannot offer claude.ai login or
subscription limits without prior approval. Retain the user's own interactive Claude Code
terminal workflow for login-based use. Use API authentication for an embedded Claude SDK
integration unless Anthropic approves another route. Do not extract Claude subscription tokens
and pass them to OpenCode or to a direct HTTP adapter.

## First implementation

OpenCode is the first multi-provider runtime, connected through the Agent Client Protocol (ACP).
This reuses Warp's existing native conversation rendering, cancellation, tool display, model
selection, and MCP transport. It avoids implementing a second tool loop for each model API.

The initial change makes `--harness opencode` resolve to `opencode acp`, enables it for ordinary
agent runs, and identifies it as OpenCode in the client. Existing Codex and Claude CLI paths
remain available. Provider credentials remain managed by the selected runtime.

This is an integration milestone. Native desktop chats now offer Warp Agent or OpenCode in
the input footer. OpenCode supplies the provider/model catalog. The selected native runtime
becomes the default for new user-created chats on this computer. Warp Agent remains the initial
default. End-to-end desktop and authenticated-provider verification are still required before
describing the fork as a finished replacement application.

## Desktop local runtime launcher

The desktop now has a separate **New local agent tab** command and a new-tab menu entry.
In **Settings > Agents > Third party CLI agents**, select OpenCode, Codex, or Claude Code.
OpenCode is the initial local preference; the choice is saved only on this computer.

The action opens a fresh terminal tab, inherits the ordinary new-tab working-directory rules,
waits for shell startup, and starts the chosen interactive CLI. It does not reuse the active
terminal's input or send a prompt to Warp's inference service. It remains available when Warp
AI is disabled. The existing Warp Agent and cloud session actions keep their existing meaning.

The runtime must already be installed in the selected shell. The settings page links to its
official setup guide and explains login/API-key support. Authentication, provider selection,
and permission prompts happen in the runtime's own UI. The launcher does not copy credentials,
edit global provider configuration, pass auto-approval flags, or silently select another agent.

This launcher remains an interactive terminal route for Codex and Claude Code logins, as well
as OpenCode. Native OpenCode chats use the separate ACP desktop bridge described below.

## Native OpenCode chats

In a new Agent chat, choose **OpenCode** in the input footer, then choose a provider/model
advertised by the installed runtime. Replies and tool activity render in the native conversation.
The footer shows connection status, errors, model selection, Stop, and connection retry.

Connect providers through OpenCode's supported `/connect` flow or API configuration first.
The app does not copy credentials into conversation data. Native chats request approval for
tools through **Allow once** and **Deny** controls; existing hard deny rules still apply.

Local history stores the runtime, working folder, provider/model ID, and OpenCode session ID.
Reopening the chat loads that session and confirms its saved model. An unavailable runtime,
folder, session, or model produces an error instead of using Warp or another provider.
Warp passive suggestions are disabled for these conversations.

This desktop bridge currently accepts plain text and local host folders. Attached context,
session forks, shared/cloud sessions, SSH/WSL execution, and queued follow-up messages are not
supported. It rejects unsupported input explicitly. Stop the current turn before sending another
message. The general terminal launcher remains available for the other runtimes.

## Required behavior for the finished product

1. A user can select a runtime and discover its models without editing source code.
2. A user can connect with an API key or the runtime's supported login flow. Unsupported
   combinations are explained in the UI.
3. Credentials are never committed or copied between unrelated runtimes. Connecting a runtime
   does not rewrite the user's global Codex or Claude configuration.
4. Provider failures remain visible. The module must not silently fall back to Warp credits or
   another paid provider.
5. New conversations use the selected default runtime. Existing conversations retain the
   runtime and model needed to resume them.
6. Tool progress, file changes, cancellation, errors, and approvals are visible in the terminal
   application, with capability differences shown where they affect a decision.
7. Adding a provider supported by OpenCode requires configuration, rather than a new Warp enum.
8. Windows PowerShell, bash, zsh, and fish behavior is verified before distributing installers.

## Work still required

- Add context attachments, queued follow-ups, and supported remote-session behavior to native chats.
- Add native Codex/Claude runtime adapters where their supported authentication and protocols allow.
  Existing login-based use remains available through the local terminal launcher.
- Complete live desktop validation of the native selector, approvals, and connection recovery.
- Decide whether local-only sessions without a Warp account are required. Current task creation,
  conversation persistence, and sharing still involve Warp services. Replacing inference alone
  does not remove those dependencies.
- Verify real authenticated turns with OpenAI, Anthropic, and Z.ai, plus cancellation, file edits,
  denied actions, provider failure, and conversation resumption.
- Windows portable and installer build scripts are available; see [packaging](windows-packaging.md).
  Complete a full release build, desktop validation and signing before distributing a release.

## Sources checked on 2026-10-26

- [Warp source and license overview](https://github.com/warpdotdev/warp)
- [Codex app-server](https://learn.chatgpt.com/docs/app-server)
- [Claude Agent SDK and authentication restrictions](https://code.claude.com/docs/en/agent-sdk/overview)
- [OpenCode ACP](https://dev.opencode.ai/docs/acp/)
- [OpenCode providers](https://docs.opencode.ai/docs/providers/)
- [Z.ai quick start](https://docs.z.ai/guides/overview/quick-start)
