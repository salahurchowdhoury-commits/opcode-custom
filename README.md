# opcode-custom

A custom Windows-friendly build of [opcode](https://github.com/getAsterisk/opcode), a Tauri desktop interface for Claude Code. This repository contains the customized source, including the patched Claude binary discovery and process-handling code in `src-tauri`.

> **Important:** This project is not affiliated with, endorsed by, or sponsored by Anthropic. Claude is a trademark of Anthropic, PBC. Use your own valid API credentials and follow the terms of the services you use.

## What is included

- Tauri 2 desktop application with a React/Vite frontend.
- Claude Code project and session management.
- Custom agents, MCP server management, analytics, and session tools.
- Patched `src-tauri/src/claude_binary.rs` for Windows Claude Code discovery and execution.
- Cross-user configuration: no personal Windows username or local API credential is required.

## Requirements (Windows 11)

Install these before building:

1. **Node.js LTS** (includes npm): <https://nodejs.org/>
2. **Git for Windows**: <https://git-scm.com/download/win>
3. **Rust via rustup**: <https://rustup.rs/>
4. **Microsoft C++ Build Tools** with the Desktop development with C++ workload: <https://visualstudio.microsoft.com/visual-cpp-build-tools/>
5. **WebView2 Runtime** (normally included with Windows 11): <https://developer.microsoft.com/microsoft-edge/webview2/>
6. **Claude Code CLI**: install it from the official Claude Code documentation, then verify that `claude` is available in a new terminal.
7. **OmniRoute** (optional): only needed for the local OmniRoute integration; install it separately if you use that feature.

Verify the main tools:

```powershell
node --version
npm --version
git --version
rustc --version
cargo --version
claude --version
```

## Configure Claude Code

Copy the example settings file to the standard Claude Code settings location:

```powershell
New-Item -ItemType Directory -Force "$env:USERPROFILE\.claude" | Out-Null
Copy-Item .\settings.example.json "$env:USERPROFILE\.claude\settings.json"
notepad "$env:USERPROFILE\.claude\settings.json"
```

Replace the three placeholder values with your own provider details:

- `ANTHROPIC_BASE_URL`: the base URL supplied by your provider.
- `ANTHROPIC_AUTH_TOKEN`: your own API key or access token.
- `ANTHROPIC_MODEL`: your provider's model or combo name.

Never commit a real key. `settings.json` belongs in your user profile and is intentionally not part of this repository.

## Install dependencies and build

From the repository root:

```powershell
npm install
npm run tauri build
```

The packaged Windows installer and executable are generated under `src-tauri\target\release\bundle\` and `src-tauri\target\release\` respectively. Build output is ignored by Git.

For frontend development:

```powershell
npm run dev
```

For a TypeScript and Rust check:

```powershell
npm run check
```

## Repository safety

- Personal credentials and machine-specific paths must not be committed.
- Use `%USERPROFILE%` / `$env:USERPROFILE` in documentation instead of a personal Windows username.
- Keep `settings.json`, `.env`, `node_modules`, and Rust `target` output local.
- If a credential is exposed, revoke it immediately and replace it.

## License

This project retains the license and notices included in the source tree. See [LICENSE](LICENSE).

## বাংলা সংক্ষিপ্ত নির্দেশনা

Windows 11-এ চালানোর পূর্ণ দ্বিভাষিক নির্দেশনার জন্য `SETUP_GUIDE.txt` দেখুন। সেখানে Node.js, Git, Rust, Claude Code CLI, API সেটিংস, এবং build/run ধাপগুলো দেওয়া আছে।
