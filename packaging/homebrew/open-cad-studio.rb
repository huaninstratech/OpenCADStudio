cask "open-cad-studio" do
  version "2026.41"
  sha256 "37f61cde861510cd83202bb854c64b16e3c1fa68187aeccf09e65395fd9d0198"

  url "https://github.com/HakanSeven12/OpenCADStudio/releases/download/v#{version}/OpenCADStudio-v#{version}-macos-arm64.dmg",
      verified: "github.com/HakanSeven12/OpenCADStudio/"
  name "Open CAD Studio"
  desc "CAD application for 2D drafting and 3D modeling that reads/writes DWG and DXF"
  homepage "https://github.com/HakanSeven12/OpenCADStudio"

  # Only an Apple Silicon (arm64) build is published.
  depends_on arch: :arm64

  app "OpenCADStudio.app"

  # The app is ad-hoc signed but not notarised (no paid Apple Developer ID),
  # so macOS asks once on first launch: System Settings > Privacy & Security >
  # Open Anyway. Homebrew installs casks only from a tap; see README.md.

  zap trash: [
    "~/Library/Application Support/OpenCADStudio",
    "~/Library/Preferences/io.github.HakanSeven12.OpenCadStudio.plist",
    "~/Library/Saved Application State/io.github.HakanSeven12.OpenCadStudio.savedState",
  ]
end
