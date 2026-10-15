# CSP lights patch: static analysis of builds 0.2.11 and 0.3.0-preview675

Status: static analysis only. Nothing was executed, loaded or disassembled.
Analysis date: 2026-10-15. Tools: a static PE analysis helper (its artifact inspection, PE resource inspection and capability report commands), plus `shasum`, `strings`, `find`, `diff` and a small Python PE parser.

The two inputs are Custom Shaders Patch (CSP) lights patch folders, each one chosen by the user and holding `dwrite.dll` and `extension/`:

- Input A: a user-selected lights patch folder, build 0.2.11
- Input B: a user-selected lights patch folder, build 0.3.0-preview675

## 1. Summary

- Both `dwrite.dll` files are PE32+ x86-64 DLLs. The helper's artifact inspection succeeded on both. Their SHA-256 digests are the identity keys used by `src/csp.rs`.
- The helper could not analyse the DLLs deeper. Its PE resource inspection refuses both files at its 64 MiB default (it works with a raised limit). Its managed-artifact inspection refuses the 0.3.0 file at its 128 MiB snapshot limit. No native disassembly provider was run.
- The version labels are confirmed by strings inside each binary (0.3.0 also in its version resource), not by the helper. The helper only reports the digest.
- The Steamworks code is partly visible. 0.3.0 contains Steam interface-version strings that 0.2.11 lacks. Neither DLL lists a Steamworks DLL in its import tables.
- The `csp_*` names are strings in both DLLs but are not in either PE export table. This contradicts the working assumption that both DLLs export them. See section 4.
- The `extension/` trees differ in 250 `diff -rq` entries (202 changed files, 48 files or directories present on one side only).

## 2. Inputs and identity

| Item | 0.2.11 | 0.3.0-preview675 |
|---|---|---|
| Folder | a user-selected lights patch folder | a user-selected lights patch folder |
| `dwrite.dll` size (bytes) | 121,008,424 | 140,377,624 |
| `dwrite.dll` SHA-256 | `6546fdf7854213dcc87a0b2bcb68155bebcc0d7892d3becb5795f310cbf24c6e` | `178e3fef2cc08ade04ba37624f277da07edb0c633b2f4af3d7695202c2e4c645` |
| Format (`file`) | PE32+ executable (DLL), x86-64 | PE32+ executable (DLL), x86-64 |
| Machine / optional header | 0x8664 / PE32+ (0x20b) | 0x8664 / PE32+ (0x20b) |
| Section count | 7 | 9 |
| PE TimeDateStamp (UTC) | 2025-06-11 22:00 | 2026-10-07 23:00 |
| File mtime (UTC) | 2025-06-11 22:08 | 2026-10-07 23:25 |
| `extension/` files | 694 | 752 |
| `extension/` directories (incl. root) | 151 | 157 |
| Version resource | ProductName "AC Custom Shaders Patch", CompanyName "AC Modders Team" | same, plus "preview675" in the version fields |

Both sizes and digests were re-checked with `stat -f %z` and `shasum -a 256` on 2026-10-15. Both match the values in `src/csp.rs`.

## 3. Helper runs

All helper commands were run from a scratch directory, not from the input folders or the project. Each command used `--format json`. The full JSON outputs were kept outside the repository.

Commands (`<helper>` is the static analysis helper, and `<dll>` is the path to the `dwrite.dll` under test):

```
<helper> inspect-artifact <dll> --format json
<helper> inspect-managed-artifact <dll> --format json
<helper> inspect-pe-resources <dll> --format json
<helper> inspect-pe-resources <dll> --max-file-bytes 268435456 --format json
<helper> inspect-binary-layout <dll> --format json
<helper> capabilities --format json
```

Results:

| Helper command | 0.2.11 | 0.3.0-preview675 |
|---|---|---|
| `inspect-artifact` | Succeeded. Provider: artifact graph, format `pe`, 1 node, 1 occurrence, 0 edges, coverage `complete`. Evidence `ev_e0f7ca75...`. Graph SHA-256 `3b575edf...`. | Succeeded. Same shape. Evidence `ev_5ff322be...`. Graph SHA-256 `a9c57db3...`. |
| `inspect-managed-artifact` | Succeeded. Provider: .NET static. Classification `not-managed`, container `pe`, reason `cli-directory-absent` (no CLI data directory, so no managed metadata or CIL). | Failed, `resource_constraint`: "Managed static inspection requires an eagerly retained PE snapshot within its input byte budget", `max_snapshot_bytes` 134,217,728 (128 MiB). The file is 140,377,624 bytes, 6,159,896 bytes over the limit. |
| `inspect-pe-resources` (default) | Failed, `resource_constraint`: exceeds `max_file_bytes` 67,108,864 (64 MiB). | Failed, same limit. |
| `inspect-pe-resources --max-file-bytes 268435456` | Succeeded. Provider: PE resources. 16 resources, 1 icon group, coverage `complete` (36 directory entries examined). | Succeeded. Same shape. |
| `inspect-binary-layout` | Failed, `capability_unavailable`: the ELF profile supports Linux x64 only. | Same. |

Notes on these results:

- `inspect-artifact` only confirms the file's format, digest and single-node structure. Its own next-step list points to the managed-artifact and binary-open probes.
- `inspect-pe-resources` with the raised limit gave the same resource layout in both files: 13 RT_ICON, 1 RT_GROUP_ICON, 1 RT_VERSION, 1 RT_MANIFEST. Icons and the version payload are inventoried, but image contents are not decoded.
- Raising `--max-file-bytes` is a per-run choice and does not change the default. The managed inspection has no equivalent flag in this version, so 0.3.0 cannot be classified by the helper with its defaults.
- The helper's capability report on this machine lists three external disassemblers: one is available but was not used here, and two are unavailable because their location variables were not set. The native-macOS and artifact-graph providers are available. The managed provider is available but was blocked by size, as above.

## 4. Static PE observations (ad-hoc parser, not the helper)

A short Python script read the PE headers, export directory and import directories of each file. It is not a verified PE tool. The results are structural observations only. No code was disassembled.

Export name table (`DWriteCreateFactory` and the vendor names are the only exports that look like the DWrite proxy):

- 0.2.11: 3 named exports: `DWriteCreateFactory`, `AmdPowerXpressRequestHighPerformance`, `NvOptimusEnablement`. Export DLL name: `accRenderingAdv.dll`.
- 0.3.0: 31 named exports: the same 3, plus 28 `ffxFsr2*`, `ffxFsr3Upscaler*`, `ffxGet*` and `ffxSafeRelease*` names. Export DLL name: `accRenderingAdv.release.dll`.
- Neither export table contains `csp_Initialize_v1`, `csp_Raycast_v1` or `csp_GetLastError_v1`.

Import tables:

| Table | 0.2.11 | 0.3.0-preview675 |
|---|---|---|
| Normal imports | 11 DLLs, 380 functions | 13 DLLs, 452 functions |
| Delay-load imports | 15 DLLs, 121 functions | 19 DLLs, 141 functions |

- Neither file has `steam_api64.dll` (or any Steamworks DLL) in its normal or delay-load import table.
- 0.3.0 delay-loads `ffx_backend_dx11_x64.dll` (FidelityFX) and `libxess_dx11.dll` (XeSS). 0.2.11 does not reference either.
- Both files import the usual Windows DLLs (KERNEL32, USER32, GDI32, ADVAPI32, ole32, d3d11, dxgi, IMM32, and others).

Section layout: 0.3.0 has two extra sections compared with 0.2.11 (9 vs 7). Section names were not compared in detail.

## 5. String observations (`strings` and byte search, not disassembly)

| String | 0.2.11 | 0.3.0 | Note |
|---|---|---|---|
| `csp_Initialize_v1`, `csp_Raycast_v1`, `csp_GetLastError_v1` | present (2 hits each) | present (2 hits each) | In `.rdata`. Not in the export table (section 4). |
| `DWriteCreateFactory` | present | present | Exported. |
| `csp_ai_get_plugin`, `csp_hands` | absent | present | 0.3.0 only. |
| `steam_api64` symbol names (`SteamAPI_Init`, `SteamUser`, `SteamUserStats`, and similar) | present (11 hits) | present (16 hits) | Both files. |
| `STEAMAPPS_INTERFACE_VERSION007/008`, `STEAMUSERSTATS_INTERFACE_VERSION011/013`, and other `STEAM*_INTERFACE_VERSION*` strings | absent | present (12 interface-version strings) | 0.3.0 only. |
| `VERIFY_STEAM_API` | absent | present | 0.3.0 only. |
| `ffxFsr2*`, `ffxFsr3*`, `libxess` | absent | present | 0.3.0 only. Matches the export and import observations. |
| `0.2.11` | present (the manifest `assemblyIdentity version="0.2.11.0"` and "Custom Shaders Patch v0.2.11") | one raw-byte match, context not checked | Not a text string in 0.3.0 as far as `strings` shows. |
| `0.3.0-preview675` | absent | present (version resource, and "Custom Shaders Patch v0.3.0-preview675") | 0.3.0 only. |

These are string-level observations. They do not show whether the code uses these strings, or how.

Steam: the `\steam_api64\SteamAPI_Init`-style strings look like names resolved at runtime, but this is a hypothesis. Since neither DLL imports `steam_api64.dll`, the Steamworks calls are most likely loaded dynamically. This is not verified.

Version labels: the labels used in `src/csp.rs` (`0.2.11`, `0.3.0-preview675`) are supported by strings in each binary. The helper did not report them.

## 6. `extension/` tree comparison

Files only (paths compared by relative path; `find -type f | sort` and `comm`):

- Common relative paths: 689.
- Only in 0.2.11: 5 files.
- Only in 0.3.0: 63 files.
- Common paths with different content (`cmp`): 202.

`diff -rq` on the two trees prints 250 lines. These are 202 "Files ... differ" lines plus 48 "Only in" lines (43 for 0.3.0, 5 for 0.2.11). The 48 "Only in" lines cover directories as well as files, so they are not the same count as the 63 and 5 file-only numbers above.

Top-level entries are identical in both trees: `config`, `internal`, `lua`, `sfx`, `shaders.zip`, `textures`, `weather`, `weather-controllers`.

Only in 0.3.0 (examples):

- `config/data_paintable.ini`, `config/module_lw.ini`, `config/rain_fx.ini`
- three helper scripts under `internal/lua-shared/` (for Steam and VR)
- `internal/plugins/libgns.dll`, `internal/plugins/openxr_loader.dll`, `internal/plugins/libai2.dll`, and one bundled vendor plugin executable for keyboard hardware
- `lua/new-modes/p2p-1v1/` (including `impl_steam.lua`, `gns.lua`), `lua/new-modes/revenant/`, `lua/new-modes/sfp-test/`
- `sfx/csp_extras_rain.bank`, `sfx/csp_extras_sfp.bank`
- `weather/base/shaders/volumetric_fog.fx` and related files

Only in 0.2.11:

- `internal/lua-apps/CspDebug/_test.ini`, `internal/lua-apps/CspDebug/_test_auto.lua`
- one bundled vendor plugin executable for lighting hardware
- `weather/base/shaders/showroom_cubemap.fx`, `weather/base/src/weather_clouds_pertrack.lua`

The `extension/` folder layout matches what the app copies into the Assetto Corsa root (`csp.rs`, `install_patch`, see section 7). Only the file list and content were compared. The Lua, INI and shader contents were not analysed.

## 7. How the app uses this analysis (`src/csp.rs`)

- `KNOWN_PATCHES` (line 23): two `(sha256, version)` pairs. These are the digests in section 2.
- `sha256_file` (line 85): streams the whole `dwrite.dll` through SHA-256 in 1 MiB chunks. Any byte change, including a re-pack of the same version, gives a different digest.
- `known_version` (line 103): exact match against `KNOWN_PATCHES`.
- `load_patch` (line 151): a folder is a lights patch only if its name is `lights-patch-v<version>` and it contains `dwrite.dll` and `extension/`. The reported version is the table value if the digest is known; otherwise it falls back to the folder name, and `known` is false.
- `installed_version` (line 199): returns the `UNRECOGNIZED` constant when the live `dwrite.dll` digest is not in the table.
- `family_of` (line 111): groups versions as `0.2` or `0.3`.
- `install_patch` (line 258) archives the live files, then copies `dwrite.dll` and `extension/` into the AC root and rolls back on failure. `uninstall_patch` (line 308) archives the live files via `archive_current` (line 208). Nothing is deleted: displaced files go to `acxs-patch-archive/` (`ARCHIVE_DIR`, line 14).

Consequence: only the two listed digests are recognised as known versions. Any other build, including a repack of one of these versions, is treated as unrecognised for installed-state reporting. Discovery falls back to the folder name for its label.

## 8. Not analysed, and what would be needed

Status after section 9. Items 1, 2, 3, 5 and 6 are partly answered. Items 4 and 7 are unchanged. The open points are listed in section 9.7.

1. Code behaviour of `csp_Initialize_v1`, `csp_Raycast_v1`, `csp_GetLastError_v1`, and how they are exposed to callers. **Partly answered (9.1):** each file has a loader function that resolves the three names at run time with `GetProcAddress`, from a module opened with `LoadLibraryW`. The names are not exported by the DLL. Open: which module the loader opens, and what the resolved functions do.
2. Steamworks use. **Partly answered (9.2):** 0.3.0 loads a Steam module at run time and resolves the `SteamAPI_*` and `SteamInternal_*` names with `GetProcAddress`. Its interface-version strings are passed to an interface accessor at 61 call sites. 0.2.11 has the strings but no code reference to them. Open: the decoded module name, the accessor, and how the interfaces are used.
3. 0.3.0 FidelityFX and XeSS code paths. **Answered for the import call graph (9.3):** which functions call the XeSS and FFX import slots, and which internal functions call the DLL's own FFX exports. Open: the logic that chooses between the paths.
4. Managed classification of 0.3.0. The helper's managed path refused the file on size. A native-only classification of 0.2.11 is done; 0.3.0 has not been classified by the helper at all. **Unchanged.**
5. Code differences between the two DLLs. **Partly answered (9.4):** exact-byte comparison of the `.pdata` function bodies. A comparison that ignores displacements needs an instruction decoder, which is not installed.
6. `extension/` file contents, including the new Steam and VR helper scripts, the `p2p-1v1` Lua mode, and the bundled `.exe`/`.dll` plugins. **Partly answered (9.5):** byte equality and file lists. The contents were not reviewed.
7. Runtime behaviour. Nothing was launched, and no game session was observed. **Unchanged.**

Tools that would be needed:

- A decompiler with PE support. Ghidra 12.1.4 is installed (its `analyzeHeadless` is under the Homebrew `libexec/support` folder) but was not run. A full decompile of the 0.3.0 file would be a long run. A targeted decompile of the functions named in section 9 is the practical option.
- An instruction decoder for displacement-independent function comparison. `capstone` is not installed.
- A third external disassembler needs a configuration variable that was not set here.
- rizin 0.9.1 and `rz-bin` were used for disassembly and full analysis (section 9.6). They are PE-aware on this machine.
- A Java and managed-code analyzer is not needed for the native DLLs.

For the managed side, a standard PE header dump tool would give an independent cross-check of the parser results in section 4. No standard PE dump tool was found on this machine. A small Python parser written for section 9 agreed with section 4 on export counts, normal and delay import counts, and the delay-import DLL names.

## 10. Reproducing

1. Verify digests and sizes:

   ```
   shasum -a 256 <input A>/dwrite.dll <input B>/dwrite.dll
   stat -f '%z %N' <input A>/dwrite.dll <input B>/dwrite.dll
   ```

   Here `<input A>` and `<input B>` are the two user-selected lights patch folders.

2. Run the helper commands in section 3 from a scratch directory.
3. Strings:

   ```
   strings -n 6 <dll> | grep -E 'csp_|STEAM[A-Z]+_INTERFACE|steam_api64'
   ```

   Strings only show that a name exists in the file. To check exports, parse the PE export directory (data directory 0) and compare its name table with the strings, as described in section 4.
4. Extension trees (write any listings to a scratch directory, not into the input folders):

   ```
   diff -rq <input A>/extension <input B>/extension
   ```

   The 250-line output, with 202 `Files ... differ` lines, is from this command. Per-file counts come from `find <dir> -type f | sort` plus `comm` on the two lists.

Do not run the helper or any other analysis from inside the lights patch folders, and do not execute the DLLs.

## 9. Deep analysis results

Run: 2026-10-15 to 2026-10-16 (local time). The input folders were only read. Every tool ran on copies in a scratch directory outside the repository. Nothing was executed. Section 8 lists what is still open.

Notation: VA = image base (0x180000000) + RVA. Both DLLs have that image base. "<copy A>" and "<copy B>" are the scratch copies of the 0.2.11 and 0.3.0 `dwrite.dll`.

### 9.0 Method and checks

- Copies were made with `cp`. Their SHA-256 values match section 2 (0.2.11 `6546fdf7...`, 0.3.0 `178e3fef...`).
- `rz-bin` gave the section tables. Python 3.9 scripts with no third-party packages parsed the PE headers, the export table, the normal and delay import tables, and the exception directory (`.pdata`). `.pdata` gives the begin and end RVA of every x64 function, so it is the source of function boundaries below.
- Reference search: for each target address, the code sections were scanned for RIP-relative 32-bit references (`lea`, `mov`, `call [rip+x]`, `jmp [rip+x]`) and the whole file for 64-bit absolute pointers. The RIP scan is heuristic. It tests every ModRM byte with mod=00 and rm=101 and accepts an immediate size of 0, 1 or 4 bytes after the displacement. Hits that matter were checked against the disassembly.
- The byte-search offsets for the `csp_` strings match the ranges in section 5 (0.2.11 `csp_GetLastError_v1` at file offset 0x1d33640, 0.3.0 `csp_Raycast_v1` at 0x22cbe08). Some `grep -b` offsets for the same strings were off by up to 36 bytes, so the byte-search values are used.
- Disassembly came from rizin (`pD` on a function range). The later rizin runs used `bin.strings=false`, because a run without it took more than 120 seconds and was stopped. The first `aa` run did not use it and took about a minute.

### 9.1 The `csp_` names: code references and how they are registered

Results:

| Name | 0.2.11 string VA | 0.2.11 code reference | 0.3.0 string VA | 0.3.0 code reference |
|---|---|---|---|---|
| `csp_Initialize_v1` | 0x181d34f00 | LEA at 0x180c8fbfb | 0x1822cd890 | LEA at 0x1815c4f46 |
| `csp_GetLastError_v1` | 0x181d34e40 | LEA at 0x180c8fc5d | 0x1822cd848 | LEA at 0x1815c4fa8 |
| `csp_Raycast_v1` | 0x181d34e88 | LEA at 0x180c8fcbf | 0x1822cd808 | LEA at 0x1815c500a |

- The addresses in this table are the LEA instruction addresses from the disassembly. The scan itself reports the ModRM byte, which is one or two bytes later.
- No 64-bit absolute pointer to any of these strings exists in either file.
- In 0.2.11 all three references lie in one function, 0x180c8f8b0 to 0x180c8fe5e (1,454 bytes). In 0.3.0 they lie in one function, 0x1815c4bd0 to 0x1815c5162 (1,426 bytes).

What the 0.2.11 function does (read from the disassembly):

1. It calls `LoadLibraryW` (an import) on a wide-string path that it builds at run time (call at 0x180c8f9bf). Where that path comes from was not traced.
2. It checks the loaded module and reports failures with these strings: "Module did not register any surfaces", "Custom physics or custom raycasting is disabled", "Module is missing", "Integrity check failed", "Failed to calculate checksum", "Failed to load module". The checksum-related strings suggest the module is verified before its functions are used. The exact check was not read.
3. It resolves the three names with `GetProcAddress` (imports, calls at 0x180c8fc05, 0x180c8fc67 and 0x180c8fcc9) and stores the results in global slots: `csp_Initialize_v1` into 0x18206b630 (store at 0x180c8fc0b), `csp_GetLastError_v1` into 0x18206b640 (store at 0x180c8fc6d), `csp_Raycast_v1` into 0x18206b620 (store at 0x180c8fccf). A missing name produces "Function ... is missing".
4. The function later calls `csp_GetLastError_v1` through the slot (`call qword [0x18206b640]`, at 0x180c8fe2d). The path around that call was not traced.
5. The only direct caller found (E8/E9 scan) is at 0x180f669b8, inside the function 0x180f65730 to 0x180f67002. That caller was not read.

The 0.3.0 loader is the same pattern: `LoadLibraryW` at 0x1815c4ce6, the same six failure strings plus "Module failed to initialize", and `GetProcAddress` calls at 0x1815c4f50, 0x1815c4fb2 and 0x1815c5014 for the same three names. Its slot stores and callers were not traced.

Registration, in short: the `csp_` names are looked up at run time by name with `GetProcAddress`, against a module opened with `LoadLibraryW`. They are not in the export table of the DLL that holds them. Section 4 asked for an explanation of that: the names are resolved in a different module than the `dwrite.dll` being inspected. The identity of that module (its file and path) was not determined.

Other `csp_` strings:

- `csp_main_menu_v1` is in both files. Neither file has a code or pointer reference to it. Its use was not determined.
- `csp_hands` (0.3.0 only) is referenced by a LEA with a REX.WR prefix at 0x1806d4d92, inside the function 0x1806d4ca0 to 0x1806d5095 (1,013 bytes). That function also holds OpenXR action names ("/user/hand/left", "/user/hand/right", "xrCreateActionSet", "Hand pose", "trigger", "thumbstick"). The name is passed as an argument, followed by the string "Hands", to a helper at 0x181d02dc0. The helper was not read.
- `csp_ai_get_plugin` (0.3.0 only) is referenced by a LEA at 0x1815c7a41, inside the function 0x1815c71d0 to 0x1815c7eb0 (3,296 bytes). This is a second, separate loader for plugins. It calls `LoadLibraryW` (at 0x1815c74c9 and 0x1815c7975), then `GetProcAddress("csp_ai_get_plugin")` (at 0x1815c7a4b). Its messages include "AI plugins: not a plugin name", "AI plugins: cannot find or copy", "... crashed starting up (exception 0x%08X)", ": no csp_ai_get_plugin export", ": built for another API version" and ": refused to start".

### 9.2 Steamworks

- Neither file imports a Steam DLL, in the normal or the delay import table. Re-checked with the parser.
- 0.2.11: the Steam strings are in `.rdata` (`SteamAPI_Init`, `SteamAPI_RegisterCallback`, `SteamAPI_UnregisterCallback`, `SteamAPI_UnregisterCallResult`, and the path-like `\steam_api64\...` strings). The scan found no code reference and no pointer to any of them. This file has no interface-version strings.
- 0.3.0, module loading: the helper at 0.3.0 VA 0x1803d0b00 calls 0x1803d0b70. That function builds a name from a 24-byte encoded buffer at 0x1822d8d30 plus two 4-byte immediates, through a decoder at 0x1803d1f40 (a key-based XOR routine). The result goes through a call at 0x18154a520 (not read) and is then passed to `LoadLibraryW` (call at 0x1803d0c61). The decoded value was not determined: the decoder was read only in part. Neither file has the plain text "steam_api64.dll" in ASCII or UTF-16. The decoded name is probably the Steam API DLL, but this is not verified.
- 0.3.0, name resolution: functions 0x1803d0e10 to 0x1803d2460 and the helpers around them call `GetProcAddress` for: `SteamAPI_GetHSteamPipe`, `SteamAPI_GetHSteamUser`, `SteamAPI_RunCallbacks`, `SteamAPI_RegisterCallback`, `SteamAPI_UnregisterCallback`, `SteamAPI_RegisterCallResult`, `SteamAPI_UnregisterCallResult`, `SteamInternal_ContextInit`, `SteamInternal_CreateInterface` (in 0x1803d1350 to 0x1803d14a6), `SteamInternal_FindOrCreateUserInterface` and `SteamInternal_SteamAPI_Init`. The string "Trying to unknown Steam function: %s" is used in the same address range, which suggests a failed lookup is reported with it (not confirmed).
- 0.3.0, interface versions: the 12 `STEAM*_INTERFACE_VERSION*` strings are referenced by LEA from 61 functions (one reference each; 61 RIP references in total). Example, function 0x1803a5840 (0x199 bytes): `lea rcx, [STEAMAPPS_INTERFACE_VERSION008]`, then `call 0x180399f40`, then `call qword [rax+0x98]`. That is, a version string is passed to a helper that returns an interface pointer, and a method at vtable offset 0x98 is called. This is consistent with how Steamworks interfaces are normally accessed. The helper and the method were not identified.
- 0.3.0, `SteamAPI_*` name references: 17 sites in 9 functions.
- Result: Steam is loaded and resolved at run time, and only the 0.3.0 file contains the code for it. The 0.2.11 file has the strings but no reference to them in code the scan can see.

### 9.3 0.3.0 FidelityFX and XeSS code paths

Import slots (from the delay import directory). Code calls these through `call qword [rip+slot]`:

- `libxess_dx11.dll`: `xessD3D11CreateContext` 0x1826d3500, `xessSetLoggingCallback` 0x1826d3508, `xessIsOptimalDriver` 0x1826d3510, `xessSetVelocityScale` 0x1826d3518, `xessD3D11Execute` 0x1826d3520, `xessGetOptimalInputResolution` 0x1826d3528, `xessD3D11Init` 0x1826d3530.
- `ffx_backend_dx11_x64.dll`: `ffxGetScratchMemorySizeDX11` 0x1826d34b0, `ffxGetDeviceDX11_Fsr31` 0x1826d34b8, `GetFfxResourceDescriptionDX11` 0x1826d34c0, `ffxGetCommandListDX11` 0x1826d34c8, `ffxGetInterfaceDX11` 0x1826d34d0.

All 21 slot references (13 FFX, 8 XeSS) are indirect calls (`ff /2`). None is a jump.

Functions that call the import slots:

| Function (VA range) | Slots called (call sites) |
|---|---|
| 0x1802e4c40 to 0x1802e5035 | `ffxGetScratchMemorySizeDX11` (1), `ffxGetDeviceDX11_Fsr31` (1), `ffxGetInterfaceDX11` (1) |
| 0x1802e5890 to 0x1802e6525 | `ffxGetCommandListDX11` (1), `GetFfxResourceDescriptionDX11` (9) |
| 0x1802f5aa0 to 0x1802f5e90 | `xessD3D11CreateContext` (1), `xessSetLoggingCallback` (1), `xessIsOptimalDriver` (1) |
| 0x1802f5fd0 to 0x1802f63bb | `xessD3D11CreateContext` (1), `xessD3D11Init` (1), `xessSetVelocityScale` (1) |
| 0x1802f63c0 to 0x1802f6811 | `xessD3D11Execute` (1) |
| 0x1802f6c00 to 0x1802f6f02 | `xessGetOptimalInputResolution` (1) |

Direct callers (E8/E9 scan) of those functions:

- 0x1802e4c40 is called from 0x1802e4c00.
- 0x1802e5890 is called from 0x1802e6530, 0x1802e6580 and 0x1802e6600.
- 0x1802f5aa0 is called from 0x1802f5a60.
- 0x1802f5fd0 has no direct caller.
- 0x1802f63c0 is called from 0x1802f6820, 0x1802f692d and 0x1802f6980.
- 0x1802f6c00 is called from 0x18035be10.

The RIP-relative references to the same slots at 0x1816f5fd2 to 0x1816f60cf (ModRM addresses) are outside the `.pdata` ranges. They look like delay-load stubs, but that was not checked.

XeSS-named code: "XeSS" is referenced from 0x1802f6655 and 0x1802f66a3 (in the Execute wrapper), from 0x1802f82de (function 0x1802f8200 to 0x1802f8787), and "XeSS12" from 0x1802f8889 and 0x1802f88d9 (function 0x1802f8790 to 0x1802f8f8d). "xess_12" is referenced from 0x1802ed995 (function 0x1802ed810 to 0x1802ee85f). The functions 0x1802f8200 and 0x1802f8790 do not call the import wrappers directly. Their role was not determined.

FFX exports of this DLL (28 names, see section 4). They are called from inside the DLL by the functions below (E8/E9 scan, 42 call sites in total):

- 0x1802e23e0, 0x1802e2720, 0x1802e2bc0, 0x1802e2ca0, 0x1802e3820, 0x1802e5040, 0x1802e5890 and 0x1802e66d0 call export addresses.
- 0x1802e4c00 calls the wrapper at 0x1802e4c40. 0x1802e6530, 0x1802e6580 and 0x1802e6600 call the wrapper at 0x1802e5890.
- Export bodies that call other exports: 0x1816ef0a0 (calls `ffxFsr2GetUpscaleRatioFromQualityMode`) and 0x1816f5c50 (calls `ffxFsr3UpscalerGetUpscaleRatioFromQualityMode`). The internal function 0x1816f51e0 calls the three `ffxSafeRelease*` exports.
- 0x181dfac40, 0x181dfad20, 0x181dfaf30 and 0x181dfaf60 call `ffxGetDeviceDX11`.

Export names and addresses: three pairs are aliases (same address). `ffxFsr2GetJitterOffset` and `ffxFsr3UpscalerGetJitterOffset` are both 0x1816eefd0. `ffxFsr2GetJitterPhaseCount` and `ffxFsr3UpscalerGetJitterPhaseCount` are both 0x1816ef060. `ffxFsr2ResourceIsNull` and `ffxFsr3UpscalerResourceIsNull` are both 0x1816ef170. Of the 28 export addresses, 13 are covered by a `.pdata` entry. The other 15 are not, and their bodies were not read (they may be leaf functions without unwind data; not determined).

No direct call from any export address range into the six import wrappers was found. Calls into the wrappers come only from the internal functions listed above.

The DLL also imports `d3d12.dll` and `POWRPROF.dll`, which 0.2.11 does not. Their use was not checked.

### 9.4 Code differences at function level

Function counts:

- `rizin` with `aa; aflc` (analysis from the entry point and exports only): 25 functions (0.2.11), 77 functions (0.3.0). These are too few to use.
- `rizin` with `aaa; aflc` (full analysis, time-limited to 1,500 s): 38,272 functions (0.2.11), 34,684 functions (0.3.0). The analysis logs have many "Cannot peek memory without specifying an address" messages from the ESIL engine. The counts are not comparable with the `.pdata` counts below.
- `.pdata` entries: 73,529 total in 0.2.11, all in `.text`. 77,645 total in 0.3.0, of which 77,626 are in `.text` and 19 are outside it.

Exact-byte comparison of `.text` functions (body = bytes from the begin RVA to the end RVA of each `.pdata` entry, SHA-256 of the body):

| Measure | 0.2.11 | 0.3.0 |
|---|---|---|
| Function entries in `.text` | 73,529 | 77,626 |
| Distinct body hashes | 70,440 | 73,516 |
| Distinct bodies that occur in both files | 3,207 | (same 3,207) |
| Function entries whose body occurs in the other file | 4,921 (6.7%) | 5,283 (6.8%) |
| Bytes in those function entries | 594,289 (2.3% of 26,036,502) | 593,761 (2.1% of 28,589,612) |

Limits: relative calls and RIP-relative displacements change when the layout changes, so exact equality undercounts shared code. No instruction decoder was available to normalise those displacements (`capstone` is not installed). The figures are a lower bound on identical code, not a similarity score. No function has a known name in either file, so these are matches by bytes only. A count of same-length functions (60,738 matches as a multiset) is a weak signal and was not used.

### 9.5 `extension/` file differences by byte equality (no content review)

Method: both trees were walked with `os.walk` (read-only). Each path present in both trees was compared by SHA-256.

| Measure | 0.2.11 | 0.3.0 |
|---|---|---|
| Files | 694 | 752 |
| Directories below the root | 150 | 156 |
| Symbolic links / other special entries | 0 / 0 | 0 / 0 |
| Common file paths | 689 | 689 |
| Byte-identical | 487 | 487 |
| Byte-different | 202 | 202 |
| Files present only in this tree | 5 | 63 |
| Directories present only in this tree | 0 | 6 |

Byte-different files by top-level folder: `config` 52, `internal` 80, `lua` 35, `sfx` 2, `weather` 30, `weather-controllers` 3 (total 202).

Files only in 0.3.0, by top-level folder: `config` 3, `internal` 27, `lua` 20, `sfx` 2, `weather` 8, `weather-controllers` 3 (total 63). Directories only in 0.3.0:

```
internal/lua-shared/physics
lua/mouse-steering/<vendor-device folder>
lua/new-modes/p2p-1v1
lua/new-modes/revenant
lua/new-modes/sfp-test
weather-controllers/base-schedule
```

Files only in 0.2.11:

```
internal/lua-apps/<debug app folder>/_test.ini
internal/lua-apps/<debug app folder>/_test_auto.lua
internal/plugins/<vendor plugin executable>
weather/base/shaders/showroom_cubemap.fx
weather/base/src/weather_clouds_pertrack.lua
```

Byte-different files (202):

<details>
<summary>Full list of byte-different files</summary>

```
config/brakedisc_fx.ini
config/car_instruments.ini
config/cars/common/materials_glass.ini
config/cars/common/materials_interior.ini
config/cars/common/materials_pbr.ini
config/cars/generic/generic.ini
config/cars/generic/generic_fixes.ini
config/chaser_camera.ini
config/chat_shortcuts.ini
config/colorful_shadowing.ini
config/custom_rendering_modes.ini
config/data_alt_mapping.ini
config/data_countries.ini
config/data_credits.txt
config/data_gbuffer.ini
config/data_helmets.ini
config/data_manifest.ini
config/dxgi_tweaks.ini
config/extra_fx.ini
config/fake_shadows_fx.ini
config/ffb_tweaks.ini
config/freer_camera.ini
config/g27_lights.ini
config/gamepad_fx.ini
config/general.ini
config/graphics_adjustments.ini
config/grass_fx.ini
config/gui.ini
config/lighting_fx.ini
config/music.ini
config/neck.ini
config/new_behaviour.ini
config/nice_screenshots.ini
config/particles_fx.ini
config/reflections_fx.ini
config/shadowed_wheels.ini
config/skidmarks_fx.ini
config/small_tweaks.ini
config/smart_mirror.ini
config/smart_shadows.ini
config/splashscreen.ini
config/surfaces_fx.ini
config/taskbar.ini
config/track_adjustments.ini
config/tracks/common/materials_glass.ini
config/tracks/common/materials_pbr.ini
config/tracks/generic/generic.ini
config/triple_custom.ini
config/tyres_fx.ini
config/vr_tweaks.ini
config/weather_fx.ini
config/windscreen_fx.ini
internal/lua-apps/AppShelf/AppShelf.lua
internal/lua-apps/AppShelf/BackgroundInstall.lua
internal/lua-apps/AppShelf/Service.lua
internal/lua-apps/AppShelf/manifest.ini
internal/lua-apps/AppShelf/utils.lua
internal/lua-apps/<debug app folder>/CarDebugWorker.lua
internal/lua-apps/<debug app folder>/<debug app folder>.lua
internal/lua-apps/<debug app folder>/manifest.ini
internal/lua-apps/Mumble/Mumble.lua
internal/lua-apps/Mumble/src/MumbleWrapper.lua
internal/lua-sdk/ac_apps/README.md
internal/lua-sdk/ac_apps/lib.lua
internal/lua-sdk/ac_background/README.md
internal/lua-sdk/ac_background/lib.lua
internal/lua-sdk/ac_background_pw/README.md
internal/lua-sdk/ac_background_pw/lib.lua
internal/lua-sdk/ac_car_cphys/README.md
internal/lua-sdk/ac_car_cphys/lib.lua
internal/lua-sdk/ac_car_scriptable_display/README.md
internal/lua-sdk/ac_car_scriptable_display/lib.lua
internal/lua-sdk/ac_chaser_camera/README.md
internal/lua-sdk/ac_chaser_camera/lib.lua
internal/lua-sdk/ac_cockpit_camera/README.md
internal/lua-sdk/ac_cockpit_camera/lib.lua
internal/lua-sdk/ac_ffb_postprocess/README.md
internal/lua-sdk/ac_ffb_postprocess/lib.lua
internal/lua-sdk/ac_joypad_assist/README.md
internal/lua-sdk/ac_joypad_assist/lib.lua
internal/lua-sdk/ac_new_modes/README.md
internal/lua-sdk/ac_new_modes/lib.lua
internal/lua-sdk/ac_online_script/README.md
internal/lua-sdk/ac_online_script/lib.lua
internal/lua-sdk/ac_pfx_fireworks/README.md
internal/lua-sdk/ac_pfx_fireworks/lib.lua
internal/lua-sdk/ac_pp_filters/README.md
internal/lua-sdk/ac_pp_filters/lib.lua
internal/lua-sdk/ac_scripted_texture/README.md
internal/lua-sdk/ac_scripted_texture/lib.lua
internal/lua-sdk/ac_splashscreen/README.md
internal/lua-sdk/ac_splashscreen/lib.lua
internal/lua-sdk/ac_tools/README.md
internal/lua-sdk/ac_tools/lib.lua
internal/lua-sdk/ac_track_script/README.md
internal/lua-sdk/ac_track_script/lib.lua
internal/lua-sdk/ac_wfx_controller/README.md
internal/lua-sdk/ac_wfx_controller/lib.lua
internal/lua-sdk/ac_wfx_impl/README.md
internal/lua-sdk/ac_wfx_impl/lib.lua
internal/lua-shared/ltn12.lua
internal/lua-shared/mime.lua
internal/lua-shared/sim/ai.lua
internal/lua-shared/sim/cars.lua
internal/lua-shared/sim/chat.lua
internal/lua-shared/sim/fireworks.lua
internal/lua-shared/sim/ghost.lua
internal/lua-shared/sim/motec.lua
internal/lua-shared/sim/surfaces.lua
internal/lua-shared/sim/traffic.lua
internal/lua-shared/sim/weather.lua
internal/lua-shared/socket.lua
internal/lua-shared/socket/ftp.lua
internal/lua-shared/socket/headers.lua
internal/lua-shared/socket/http.lua
internal/lua-shared/socket/mbox.lua
internal/lua-shared/socket/smtp.lua
internal/lua-shared/socket/tp.lua
internal/lua-shared/socket/url.lua
internal/lua-shared/ui/virtualizing.lua
internal/lua-shared/utils/diagnostics.lua
internal/lua-shared/utils/ioext.lua
internal/lua-shared/utils/playing.lua
internal/lua-shared/utils/signing.lua
internal/lua-shared/utils/steam.lua
internal/lua-shared/vr/grab.lua
internal/lua-shared/web/browser.lua
internal/plugins/<vendor plugin executable>
internal/plugins/<vendor plugin executable>
internal/plugins/<vendor plugin executable>
internal/plugins/<vendor plugin executable>
internal/plugins/<vendor plugin executable>
lua/cars/android_auto/apps/youtube/youtube.lua
lua/chaser-camera/arcade-mode-2/camera.lua
lua/chaser-camera/arcade-mode/camera.lua
lua/chaser-camera/base/camera.lua
lua/chaser-camera/base/manifest.ini
lua/chaser-camera/drone/camera.lua
lua/cockpit-camera/default/cockpit.lua
lua/cockpit-camera/default/manifest.ini
lua/cockpit-camera/default/settings.ini
lua/fireworks/holidays/fireworks.lua
lua/joypad-assist/mobile/module_render.lua
lua/loading-screen/default/splashscreen.lua
lua/new-modes/driver-interview/description.txt
lua/new-modes/driver-interview/mode.lua
lua/new-modes/driver-interview/settings.ini
lua/new-modes/frozen-drive/manifest.ini
lua/new-modes/overtake/mode.lua
lua/new-modes/rally/manifest.ini
lua/new-modes/splitscreen/manifest.ini
lua/new-modes/splitscreen/mode.lua
lua/new-modes/splitscreen/settings.ini
lua/new-modes/traffic/description.txt
lua/new-modes/traffic/mode.lua
lua/new-modes/walking-out/manifest.ini
lua/pp-filters/cartoon/filter.fx
lua/pp-filters/cartoon/filter.lua
lua/pp-filters/curved/filter.fx
lua/pp-filters/curved/manifest.ini
lua/pp-filters/planetarium/filter.lua
lua/pp-filters/tunnel/filter.lua
lua/pp-filters/vintage/filter.fx
lua/pp-filters/vintage/filter.lua
lua/tools/csp-traffic-tool/src/simulation/CarsList.lua
lua/tools/csp-traffic-tool/src/simulation/TrafficCarPhysics.lua
lua/tools/csp-traffic-tool/src/simulation/TrafficSimulation.lua
sfx/GUIDs.txt
sfx/csp_extras.bank
weather-controllers/base-live/controller.lua
weather-controllers/base-live/src/services/open_meteo.lua
weather-controllers/base/controller.lua
weather/base/manifest.ini
weather/base/settings.ini
weather/base/shaders/aurora_apply.fx
weather/base/shaders/aurora_base.hlsl
weather/base/shaders/aurora_blur_high.fx
weather/base/shaders/clouds.fx
weather/base/shaders/lightning.fx
weather/base/shaders/pp_bloom_downsample.fx
weather/base/shaders/pp_final.fx
weather/base/shaders/pp_gamma.fx
weather/base/shaders/rainhaze_apply.fx
weather/base/shaders/rainhaze_base.fx
weather/base/src/audio.lua
weather/base/src/conditions_converter.lua
weather/base/src/consts.lua
weather/base/src/light_pollution.lua
weather/base/src/render.lua
weather/base/src/render_aurora.lua
weather/base/src/render_clouds.lua
weather/base/src/render_eclipse.lua
weather/base/src/render_lightning.lua
weather/base/src/render_linear.lua
weather/base/src/render_postprocessing.lua
weather/base/src/render_rain.lua
weather/base/src/showroom_mode.lua
weather/base/src/utils.lua
weather/base/src/weather_application.lua
weather/base/src/weather_clouds.lua
weather/base/src/weather_clouds_types.lua
weather/base/weather.lua
```

</details>

Files only in 0.3.0 (63):

<details>
<summary>Full list of files only in 0.3.0</summary>

```
config/data_paintable.ini
config/module_lw.ini
config/rain_fx.ini
internal/lua-shared/physics/automated-manual-transmission.lua
internal/lua-shared/physics/automatic-transmission-utils.lua
internal/lua-shared/physics/automatic-transmission.lua
internal/lua-shared/physics/rnpd-shifting.lua
internal/lua-shared/promise.lua
internal/lua-shared/sim/altonline.lua
internal/lua-shared/sim/cspdebug.lua
internal/lua-shared/sim/di.lua
internal/lua-shared/sim/events.lua
internal/lua-shared/sim/plates.lua
internal/lua-shared/ui/base.lua
internal/lua-shared/ui/components.lua
internal/lua-shared/ui/replay.lua
internal/lua-shared/unstable/render-special.lua
internal/lua-shared/utils/kn5.lua
internal/lua-shared/utils/luasteam.lua
internal/lua-shared/utils/ntp.lua
internal/lua-shared/utils/<vendor-named utility>.lua
internal/lua-shared/utils/oswindow.lua
internal/lua-shared/utils/parcel.lua
internal/lua-shared/vr/steamvr.lua
internal/lua-shared/vr/vrmisc.lua
internal/plugins/<vendor plugin executable>
internal/plugins/libai2.dll
internal/plugins/libcurrentlyplaying.dll
internal/plugins/<networking plugin DLL>
internal/plugins/openxr_loader.dll
lua/chaser-camera/base/settings.ini
lua/mouse-steering/<vendor-device folder>/assist.lua
lua/mouse-steering/<vendor-device folder>/manifest.ini
lua/mouse-steering/<vendor-device folder>/settings.ini
lua/new-modes/p2p-1v1/description.txt
lua/new-modes/p2p-1v1/gns.lua
lua/new-modes/p2p-1v1/impl_steam.lua
lua/new-modes/p2p-1v1/impl_steam_worker.lua
lua/new-modes/p2p-1v1/manifest.ini
lua/new-modes/p2p-1v1/mode.lua
lua/new-modes/p2p-1v1/settings.ini
lua/new-modes/p2p-1v1/utils.lua
lua/new-modes/revenant/description.txt
lua/new-modes/revenant/manifest.ini
lua/new-modes/revenant/mode.lua
lua/new-modes/revenant/settings.ini
lua/new-modes/sfp-test/description.txt
lua/new-modes/sfp-test/manifest.ini
lua/new-modes/sfp-test/mode.lua
lua/new-modes/sfp-test/settings.ini
sfx/csp_extras_rain.bank
sfx/csp_extras_sfp.bank
weather-controllers/base-schedule/controller.lua
weather-controllers/base-schedule/manifest.ini
weather-controllers/base-schedule/settings.ini
weather/base/shaders/pp_virtual_mirror.fx
weather/base/shaders/rainhaze_blur.fx
weather/base/shaders/volumetric_fog.fx
weather/base/shaders/volumetric_fog_apply.fx
weather/base/src/render_volumetric_fog.lua
weather/base/src/showroom_gausplats_mode.lua
weather/base/src/weather_clouds_layer.lua
weather/base/src/weather_clouds_utils.lua
```

</details>

Names of vendor-supplied executables and some folders were replaced with `<...>` placeholders in these lists. The file contents were not compared.

### 9.6 Commands run and results

All commands ran in the scratch directory. `<copy A>` and `<copy B>` are the scratch copies.

1. `shasum -a 256` on the two copies. Results match section 2.
2. `rz-bin -S <copy>` (both). 0.2.11: 7 sections, `.text` first, `.rdata`, `.data`, `.pdata`, `_RDATA`, `.rsrc`, `.reloc`. 0.3.0: 9 sections, including `.mdata` (code, 0x2200 bytes) and `.fptable`.
3. `rz-bin -zz <copy>` (both). Returned no usable strings on this build, so the next step was used.
4. `LC_ALL=C grep -aobE '<csp, STEAM, ffx and xess patterns>' <copy>` (both). Found the names listed in section 5. Offsets were later checked with a byte search (see 9.0).
5. `python3 -I pe_basic.py <copy A> <copy B>` (parser script over the PE structures). Results: exports (3 and 31 names), normal imports (380 and 452 functions), delay imports (121 and 141 functions), `.pdata` entry counts, the three `csp_` names absent from exports, no Steam DLL in either import table, delay imports `libxess_dx11.dll` (7 functions) and `ffx_backend_dx11_x64.dll` (5 functions) in 0.3.0 only.
6. `python3 -I xref_scan.py <copy> <out.json> <names...>` (both, several name sets). Results in 9.1 to 9.3.
7. `python3 -I refs.py <copy> rip:<VA list> call:<VA list>`. Results: loader callers (9.1), slot callers (9.3), slot stores (9.1).
8. `rizin -e bin.strings=false -e scr.color=0 -qc 'pD <len> @ <VA>' <copy>` (several runs). Disassembly of the loader functions (0.2.11 and 0.3.0), the `csp_hands` and AI-plugin functions, the Steam functions (0.3.0 0x1803d0b00 to 0x1803d2460), and one Steam interface function (0x1803a5840). One earlier run was stopped (exit 144) after more than 120 seconds, when it loaded the file without `bin.strings=false`.
9. `rizin ... -qc 'aa; aflc'` (both, background, 1,500 s limit). 25 and 77 functions.
10. `rizin ... -qc 'aaa; aflc'` (both, background, 1,500 s limit). 38,272 and 34,684 functions. Finished before the limit, with ESIL messages.
11. `python3 -I delay_slots.py <copy B> libxess ffx_backend` (0.3.0). Slot addresses in 9.3.
12. `python3 -I` script over the 0.3.0 `.text` (E8/E9 call scan and grouping of callers, and a check of `ff /2` against `ff /4` for slot calls). Result: 27 indirect `call` slot references, no jumps.
13. `python3 -I func_compare.py` (both). Exact-byte function comparison, results in 9.4.
14. `python3 -I ext_compare.py` (both trees, read-only). `os.walk` plus SHA-256 per common path. Results in 9.5.
15. `ps -eo ...` and `pkill -f` to stop the stalled rizin run in step 8.
16. Tool checks: `which rizin rz-bin cutter ghidraRun`. `cutter` and `ghidraRun` exist. `cutter-cli` is not on PATH. Ghidra 12.1.4 is installed in the Homebrew Cellar, and its `analyzeHeadless` is under its `libexec/support` folder. Ghidra was not run. `python3 -c 'import capstone'` failed (not installed).

### 9.7 Not determined

- The file or path that the 0.2.11 and 0.3.0 `csp_` loaders open, and whether that module is in either lights patch folder.
- The meaning of the 0.2.11 caller at 0x180f65730, and the values it passes to the loader.
- The decoded name that the 0.3.0 Steam bridge passes to `LoadLibraryW`. The decoder at 0x1803d1f40 was not read in full.
- The helper at 0x180399f40 and the method at vtable offset 0x98 used by the Steam interface functions.
- The callers of the 0.3.0 AI-plugin loader, and the plugins it loads.
- The use of `csp_hands` and `csp_main_menu_v1`.
- Which code chooses between the FFX and XeSS paths, and what the XeSS-named functions do.
- The bodies of the 15 FFX exports that are not covered by `.pdata`.
- Any function-level match beyond exact bytes. This needs an instruction decoder (not installed here).
- Whether the full rizin analysis is complete. Its counts (38,272 and 34,684) are far below the `.pdata` counts, and it logged many ESIL messages.
- Runtime behaviour. Nothing was launched.
