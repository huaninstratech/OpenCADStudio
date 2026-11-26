# OpenCADStudio - Unreleased (2026-11-22 to Present)

> **Summary:** Faster polygon fills, polyline area calculations, and concave XCLIP hatch clipping. No donation prompt or sponsor section, and every update and download link leads to this fork.

### 🧭 This fork

* **No donation prompt or sponsor section.** The startup donation prompt, the `DONATE` command, the supporters list, the Start page's Sponsors section and the website's sponsor button are gone, and so is the sponsor's analytics script that the web build loaded.
* **Updates and downloads lead to this fork.** The update notice, the `CHANGELOG` command, the Start page's desktop button, the `WEBVERSION` command and the website's download page open this fork's releases and web build instead of upstream's.
* **Upstream through v2026.47.** Includes upstream's changes of v2026.45, v2026.46 and v2026.47; their pages are in `changelogs/`.
* **The web build loads again.** It asked for its program and data under `/app/`, an address that only exists on upstream's domain, so the page stayed on its loading screen; it now asks under this fork's page, `/OpenCADStudio/app/`.

### ⚡ Performance

* **Faster polygon fills, area calculations, and XCLIP.** Fill self-crossing and overlapping rings and rings with touching holes faster. Measure self-crossing polyline area and clip hatches to concave XCLIP boundaries with less geometry work. Median times fell by 16.3% to 88.8% across five Windows release benchmarks.
