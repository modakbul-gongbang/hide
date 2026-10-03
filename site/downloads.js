const RELEASES_URL = "https://github.com/modakbul-gongbang/hide/releases";
const LATEST_API = "https://api.github.com/repos/modakbul-gongbang/hide/releases/latest";
const PLATFORMS = Object.freeze({
  "macos-arm64": { suffix: "macos-arm64.zip", label: "macOS · Apple Silicon (ARM64)" },
  "windows-x64": { suffix: "windows-x64.zip", label: "Windows · Intel or AMD (x64)" },
  "linux-x64": { suffix: "linux-x64.tar.gz", label: "Linux · Intel or AMD (x64)" },
});

/** A public, stable, complete release is the only source of archive links. */
export function validateRelease(release) {
  if (!release || release.draft !== false || release.prerelease !== false
    || typeof release.tag_name !== "string"
    || release.tag_name.trim() !== release.tag_name
    || !/^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(release.tag_name)) {
    return { status: "unavailable", reason: "not-stable" };
  }
  const tag = release.tag_name;
  const releaseUrl = `${RELEASES_URL}/tag/${tag}`;
  if (release.html_url !== releaseUrl || !Array.isArray(release.assets)) {
    return { status: "unavailable", reason: "invalid-release" };
  }
  const downloads = {};
  for (const [platform, { suffix, label }] of Object.entries(PLATFORMS)) {
    const name = `hide-${tag}-${suffix}`;
    const urls = [];
    for (const assetName of [name, `${name}.sha256`]) {
      const matches = release.assets.filter(asset => asset?.name === assetName);
      const asset = matches[0];
      const expectedUrl = `${RELEASES_URL}/download/${tag}/${assetName}`;
      if (matches.length !== 1 || asset.state !== "uploaded"
        || !Number.isSafeInteger(asset.size) || asset.size <= 0
        || asset.browser_download_url !== expectedUrl) {
        return { status: "unavailable", reason: "incomplete-assets" };
      }
      urls.push(expectedUrl);
    }
    downloads[platform] = { label, archiveUrl: urls[0], checksumUrl: urls[1] };
  }
  return { status: "ready", tag, releaseUrl, downloads };
}

function mountDownloads() {
  const os = document.getElementById("os");
  const cpu = document.getElementById("cpu");
  const hint = document.getElementById("platform-hint");
  const title = document.getElementById("release-title");
  const detail = document.getElementById("release-detail");
  const region = document.getElementById("release-status");
  const validated = document.getElementById("validated-download");
  const version = document.getElementById("release-version");
  const archive = document.getElementById("archive-link");
  const checksum = document.getElementById("checksum-link");
  const notes = document.getElementById("version-link");
  const retry = document.getElementById("retry-release");
  let state = { status: "loading" };
  let request = null;

  // OS is only a hint. Browser identity cannot establish the processor.
  const reported = navigator.userAgentData?.platform || navigator.platform || "";
  const osHint = /mac/i.test(reported) ? "macos"
    : /win/i.test(reported) ? "windows"
      : /linux/i.test(reported) ? "linux" : "";
  if (osHint) {
    os.value = osHint;
    hint.textContent = "Your browser suggested an OS. Confirm it and choose your processor; either can be changed.";
  }

  function render() {
    validated.hidden = true;
    for (const link of [archive, checksum, notes]) link.removeAttribute("href");
    retry.hidden = state.status === "loading";
    retry.disabled = state.status === "loading";
    region.setAttribute("aria-busy", String(state.status === "loading"));
    if (state.status === "loading") {
      title.textContent = "Checking the latest release…";
      detail.textContent = "Looking for a complete stable release on GitHub. You can also use Releases or build from source.";
    } else if (state.status === "error") {
      title.textContent = "Couldn't check downloads.";
      detail.textContent = "Try again, browse GitHub Releases, or follow the source-install guide.";
    } else if (state.status === "unavailable") {
      title.textContent = "No complete stable download is available.";
      detail.textContent = "Check GitHub Releases for updates, or build from source with the installation guide.";
    } else {
      const selected = state.downloads[`${os.value}-${cpu.value}`];
      if (!os.value || !cpu.value) {
        title.textContent = `Stable release ${state.tag} is available.`;
        detail.textContent = "Choose your operating system and processor to see its archive and checksum.";
      } else if (!selected) {
        title.textContent = "No package for this setup.";
        detail.textContent = "Packages cover macOS Apple Silicon, Windows x64 and Linux x64. Check the installation guide for requirements.";
      } else {
        title.textContent = "Your download is ready.";
        detail.textContent = "Download the archive and its SHA-256 file from the same release. Follow the installation guide to verify the checksum.";
        version.textContent = `${state.tag} · ${selected.label}`;
        archive.href = selected.archiveUrl;
        archive.firstChild.textContent = `Download for ${os.options[os.selectedIndex].text} `;
        checksum.href = selected.checksumUrl;
        notes.href = state.releaseUrl;
        notes.firstChild.textContent = `Release notes for ${state.tag} `;
        validated.hidden = false;
      }
    }
  }

  async function load() {
    if (request) return;
    request = new AbortController();
    const active = request;
    const timer = setTimeout(() => active.abort(), 10_000);
    state = { status: "loading" };
    render();
    try {
      const response = await fetch(LATEST_API, {
        signal: active.signal,
        credentials: "omit",
        referrerPolicy: "no-referrer",
        cache: "no-store",
        headers: { Accept: "application/vnd.github+json" },
      });
      if (response.status === 404) {
        state = { status: "unavailable", reason: "not-found" };
      } else if (!response.ok) {
        state = { status: "error" };
      } else {
        state = validateRelease(await response.json());
      }
    } catch {
      state = { status: "error" };
    } finally {
      clearTimeout(timer);
      request = null;
      render();
    }
  }

  os.addEventListener("change", render);
  cpu.addEventListener("change", render);
  retry.addEventListener("click", load);
  window.addEventListener("pagehide", () => request?.abort());
  load();
}

if (typeof document !== "undefined") mountDownloads();
