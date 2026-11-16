# Third-Party Licenses & Notices

FreeView is licensed under the [MIT License](LICENSE).  
This project incorporates or redistributes the following third-party software, libraries, fonts, and binaries, each subject to its own license terms.

---

## 1. Bundled Fonts

### IBM Plex Sans KR (`@fontsource/ibm-plex-sans-kr` v5.3.0)
* **License**: SIL Open Font License 1.1 (`OFL-1.1`)
* **Copyright**: `Copyright 2018 IBM Corp. All rights reserved.`
* **Source**: <https://github.com/fontsource/font-files>

<details>
<summary>SIL OPEN FONT LICENSE Version 1.1 (Full Text)</summary>

```text
Copyright 2018 IBM Corp. All rights reserved.

-----------------------------------------------------------
SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007
-----------------------------------------------------------

PREAMBLE
The goals of the Open Font License (OFL) are to stimulate worldwide
development of collaborative font projects, to support the font creation
efforts of academic and linguistic communities, and to provide a free and
open framework in which fonts may be shared and improved in partnership
with others.

The OFL allows the licensed fonts to be used, studied, modified and
redistributed freely as long as they are not sold by themselves. The
fonts, including any derivative works, can be bundled, embedded,
redistributed and/or sold with any software provided that any reserved
names are not used by derivative works. The fonts and derivatives,
however, cannot be released under any other type of license. The
requirement for fonts to remain under this license does not apply
to any document created using the fonts or their derivatives.

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this license and clearly marked as such. This may
include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the
copyright statement(s).

"Original Version" refers to the collection of Font Software components as
distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting,
or substituting -- in part or in whole -- any of the components of the
Original Version, by changing formats or by porting the Font Software to a
new environment.

"Author" refers to any designer, engineer, programmer, technical
writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS
Permission is hereby granted, free of charge, to any person obtaining
a copy of the Font Software, to use, study, copy, merge, embed, modify,
redistribute, and sell modified and unmodified copies of the Font
Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components,
in Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled,
redistributed and/or sold with any software, provided that each copy
contains the above copyright notice and this license. These can be
included either as stand-alone text files, human-readable headers or
in the appropriate machine-readable metadata fields within text or
binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font
Name(s) unless explicit written permission is granted by the corresponding
Copyright Holder. This restriction only applies to the primary font name as
presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font
Software shall not be used to promote, endorse or advertise any
Modified Version, except to acknowledge the contribution(s) of the
Copyright Holder(s) and the Author(s) or with their explicit written
permission.

5) The Font Software, modified or unmodified, in part or in whole,
must be distributed entirely under this license, and must not be
distributed under any other license. The requirement for fonts to
remain under this license does not apply to any document created
using the Font Software.

TERMINATION
This license becomes null and void if any of the above conditions are
not met.

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT
OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM
OTHER DEALINGS IN THE FONT SOFTWARE.
```

</details>

---

## 2. Redistributed Binaries

### Microsoft Edge WebView2 Loader (`WebView2Loader.dll`)
* **Copyright**: Microsoft Corporation
* **License**: Microsoft WebView2 SDK Redistributable License Terms (proprietary redistributable component, not covered by the MIT License of this repository)
* **Reference**: <https://learn.microsoft.com/en-us/microsoft-edge/webview2/>

---

## 3. Rust Dependencies (`Cargo.toml` & Transitive)

### Direct Dependencies
| Crate | Version | License | Repository |
| :--- | :--- | :--- | :--- |
| `tauri` | 2.12.5 | `Apache-2.0 OR MIT` | <https://github.com/tauri-apps/tauri> |
| `tauri-build` | 2.7.4 | `Apache-2.0 OR MIT` | <https://github.com/tauri-apps/tauri> |
| `tauri-plugin-single-instance` | 2.5.5 | `Apache-2.0 OR MIT` | <https://github.com/tauri-apps/plugins-workspace> |
| `tokio` | 1.54.1 | `MIT` | <https://github.com/tokio-rs/tokio> |
| `hickory-resolver` | 0.24.4 | `MIT OR Apache-2.0` | <https://github.com/hickory-dns/hickory-dns> |
| `winreg` | 0.56.0 | `MIT` | <https://github.com/gentoo90/winreg-rs> |
| `serde` | 1.0.229 | `MIT OR Apache-2.0` | <https://github.com/serde-rs/serde> |
| `serde_json` | 1.0.154 | `MIT OR Apache-2.0` | <https://github.com/serde-rs/json> |
| `windows-sys` | 0.59.0 | `MIT OR Apache-2.0` | <https://github.com/microsoft/windows-rs> |
| `windows-core` | 0.62.2 | `MIT OR Apache-2.0` | <https://github.com/microsoft/windows-rs> |
| `webview2-com` | 0.39.1 | `MIT` | <https://github.com/wravery/webview2-rs> |
| `winresource` | 0.1.34 | `MIT` | <https://github.com/BenjaminRi/winresource> |

### Notable Transitive Dependencies (`MPL-2.0`)
The following transitive crates are licensed under the Mozilla Public License 2.0 (`MPL-2.0`). Their source files remain unmodified and are used purely as library dependencies:

| Crate | Version | License | Repository |
| :--- | :--- | :--- | :--- |
| `cssparser` | 0.37.0, 0.38.0 | `MPL-2.0` | <https://github.com/servo/rust-cssparser> |
| `cssparser-macros` | 0.7.1 | `MPL-2.0` | <https://github.com/servo/rust-cssparser> |
| `selectors` | 0.38.0, 0.41.0 | `MPL-2.0` | <https://github.com/servo/stylo> |
| `dtoa-short` | 0.3.5 | `MPL-2.0` | <https://github.com/upsuper/dtoa-short> |
| `option-ext` | 0.2.0 | `MPL-2.0` | <https://github.com/soc/option-ext> |

Other transitive Rust crates in `Cargo.lock` are licensed under permissive licenses (`MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `0BSD`, `Unlicense`, `CC0-1.0`, `Unicode-3.0`), or multi-licensed with `MIT`/`Apache-2.0` options.

---

## 4. Frontend Runtime Dependencies (`ui/package.json`)

| Package | Version | License | Repository |
| :--- | :--- | :--- | :--- |
| `@fontsource/ibm-plex-sans-kr` | 5.3.0 | `OFL-1.1` | <https://github.com/fontsource/font-files> |
| `@tauri-apps/api` | 2.12.4 | `Apache-2.0 OR MIT` | <https://github.com/tauri-apps/tauri> |
| `lucide-react` | 0.475.0 | `ISC` | <https://github.com/lucide-icons/lucide> |
| `react` | 19.3.1 | `MIT` | <https://github.com/react/react> |
| `react-dom` | 19.3.1 | `MIT` | <https://github.com/react/react> |
