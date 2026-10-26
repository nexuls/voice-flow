# Third-party notices

## Speech engine and model

| Component                       | Source                                                 | License                        | How it is used                                                       |
| ------------------------------- | ------------------------------------------------------ | ------------------------------ | -------------------------------------------------------------------- |
| Needle engine (`libneedle.a`)   | huggingface.co/Cactus-Compute/needle3 @ `d6c7d6a2e965` | Apache-2.0                     | Statically linked into the app binary                                |
| Cactus Whistle (`whistle.cact`) | huggingface.co/Cactus-Compute/whistle @ `741118687e1f` | Apache-2.0                     | Downloaded by the user at runtime, never bundled                     |
| LLVM libc++ / libc++abi         | system toolchain                                       | Apache-2.0 WITH LLVM-exception | Statically linked on Linux (the exception waives binary attribution) |

The Apache-2.0 texts from both Cactus repositories are in
`src-tauri/licenses/` and are installed with the app as bundle resources.
Neither repository ships a `NOTICE` file at the pinned revisions.

## Rust and JavaScript dependencies

Reviewed with `cargo metadata` and the installed `node_modules` (milestone 10).

- **Rust (resolved for Linux):** almost all crates are MIT and/or Apache-2.0 (or
  similar permissive: BSD, ISC, Zlib, Unicode-3.0, BSL-1.0, CDLA-Permissive-2.0).
  - MPL-2.0 (file-level copyleft, used unmodified): `cssparser`,
    `cssparser-macros`, `selectors`, `dtoa-short` (through Tauri's HTML
    tooling) and `option-ext` (through `dirs`). Their source is available on
    crates.io. Modifying those files would require publishing the changes.
  - `r-efi` is offered as MIT OR Apache-2.0 OR LGPL-2.1-or-later. MIT is used.
  - No GPL/AGPL dependencies.
- **JavaScript:** shipped code (React, Radix, lucide, Tailwind output) is MIT, ISC or
  Apache-2.0. `lightningcss` (MPL-2.0) is a build tool only and is not shipped.

For a release, generate a complete attribution file, for example with
`cargo about generate`, and include it next to these notices.
