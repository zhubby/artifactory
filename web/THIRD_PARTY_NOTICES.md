# Third-party typography and UI

Artifactory uses locally bundled Inter Variable and IBM Plex Mono. Both font
families are distributed under the SIL Open Font License 1.1; the complete
licenses accompany the pinned `@fontsource-variable/inter` and
`@fontsource/ibm-plex-mono` packages in `node_modules`.

The interface uses Radix UI (MIT), Tailwind CSS (MIT), and Lucide (ISC).
Dashboard charts use Recharts 3.11.1 (MIT) and react-is (MIT).
Package versions and dependency licenses are recorded by npm in
`package-lock.json` and the installed packages.

The November 15 redesign removed the adapted React Bits components and GSAP.
Their source and assets are no longer included in the application bundle.
