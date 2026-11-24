<!--
[INPUT]: 固定版本的字体、界面组件和终端依赖许可。
[OUTPUT]: 提供随控制台分发的版权和许可声明。
[POS]: Web 依赖的第三方归属入口。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Third-party typography and UI

Artifactory uses locally bundled Inter Variable and IBM Plex Mono. Both font
families are distributed under the SIL Open Font License 1.1; the complete
licenses accompany the pinned `@fontsource-variable/inter` and
`@fontsource/ibm-plex-mono` packages in `node_modules`.

The interface uses Radix UI (MIT), Tailwind CSS (MIT), and Lucide (ISC).
Dashboard charts use Recharts 3.11.1 (MIT) and react-is (MIT).
Package versions and dependency licenses are recorded by npm in
`package-lock.json` and the installed packages.

Device terminals use xterm.js 6.0.0 and its Fit addon 0.11.0 under the MIT License.

Copyright (c) 2017-2019, The xterm.js authors (https://github.com/xtermjs/xterm.js)
Copyright (c) 2014-2016, SourceLair Private Company (https://www.sourcelair.com)
Copyright (c) 2012-2013, Christopher Jeffrey (https://github.com/chjj/)
Copyright (c) 2019, The xterm.js authors (Fit addon)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.

The November 15 redesign removed the adapted React Bits components and GSAP.
Their source and assets are no longer included in the application bundle.
