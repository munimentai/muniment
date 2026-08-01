# Third-party notices

## Bundled frontend code and fonts

The built desktop bundle carries the following code with no embedded license comment. This document provides the notice for that code.

- `marked` 18.0.7 — MIT
- `dompurify` 3.4.12 — MPL-2.0 OR Apache-2.0
- `diff2html` 3.4.56 — MIT
- `svelte` 5.56.4 — MIT
- `@tauri-apps/api` 2.11.1 — Apache-2.0 OR MIT
- `@tauri-apps/plugin-dialog` 2.7.1 — MIT OR Apache-2.0
- `@tauri-apps/plugin-global-shortcut` 2.3.2 — MIT OR Apache-2.0
- `@tauri-apps/plugin-opener` 2.5.4 — MIT OR Apache-2.0

The bundle also carries these font files:

- `src/fonts/SchibstedGrotesk-latin.woff2`
- `src/fonts/SchibstedGrotesk-latin-ext.woff2`
- `src/fonts/CommitMono-VF.woff2`

Both Schibsted Grotesk and Commit Mono use SIL OFL 1.1. Their license texts are `third-party-notices/SchibstedGrotesk-OFL.txt` and `third-party-notices/CommitMono-LICENSE.txt`.

## Playwright relay core

Portions of `browser-control/src/index.ts` are derived from Playwright at commit `2bcd8f21ad032744763f2f0c7ba6e7006e13fa11`.

Playwright  
Copyright (c) Microsoft Corporation

Playwright is licensed under the Apache License, Version 2.0. The applicable license text is included at `browser-control/LICENSE`. Playwright's upstream NOTICE also states that it contains code derived from the Puppeteer project, available under the Apache License 2.0.

## Kokoro read-aloud foundation

Muniment's planned on-device read-aloud foundation uses the Kokoro v1.0 model
and voice-style data from Hexgrad/Kokoro-82M, converted and published in the
`thewh1teagle/kokoro-onnx` `model-files-v1.0` release. The model and voices are
licensed under the Apache License, Version 2.0. The release distribution must
include that license and any upstream notices.

The native inference foundation uses Microsoft ONNX Runtime v1.20.1 and refers
to portions of `thewh1teagle/kokoro-onnx` at commit
`6843c53fc280ab130b7a8d206ebd3407e094efdc`. Both are MIT-licensed.

Copyright (c) Microsoft Corporation

Copyright (c) 2025 github.com/thewh1teagle

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

English grapheme-to-phoneme conversion uses CPython 3.12.8 (PSF-2.0),
phonemizer-fork 3.3.1 (GPL-3.0), and espeakng-loader 0.2.4 with its bundled
eSpeak NG 1.52.0 shared library and data (eSpeak NG is GPL-3.0-or-later).
Distribution must include the applicable licenses and upstream notices and a
GPL-compliant complete-corresponding-source offer or delivery. The
espeakng-loader 0.2.4 wheel metadata declares no license; redistribution is a
release legal-review gate. Exact package artifacts and transitive dependencies
are fixed in the `kokoro-onnx` commit's `uv.lock`. See
<https://pypi.org/project/phonemizer-fork/3.3.1/>,
<https://pypi.org/project/espeakng-loader/0.2.4/>, and
<https://github.com/espeak-ng/espeak-ng/tree/1.52.0>.
