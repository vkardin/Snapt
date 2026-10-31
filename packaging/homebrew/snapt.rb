cask "snapt" do
  arch arm: "aarch64", intel: "x64"

  version "2.5.0"
  sha256 arm:   "b13d831d065c65a4b4746fc21f2079aa6f059a765a9463909b06630d0c3ce0c6",
         intel: "9d2d15a535c37a055278cebff2c279041a0decb8525e5c1f1dc1abddb5c87e65"

  url "https://github.com/vkardin/snapt/releases/download/v#{version}/Snapt_#{version}_#{arch}.dmg"
  name "Snapt"
  desc "Fast, lightweight, open-source screenshot tool"
  homepage "https://vkardin.github.io/snapt/"

  livecheck do
    url :url
    strategy :github_latest
  end

  # Snapt updates itself from GitHub Releases.
  auto_updates true
  depends_on macos: ">= :big_sur"

  app "Snapt.app"

  uninstall launchctl: "io.github.vkardin.snapt",
            quit:      "io.github.vkardin.snapt"

  zap trash: [
    "~/Library/Application Support/io.github.vkardin.Snapt",
    "~/Library/LaunchAgents/io.github.vkardin.snapt.plist",
  ]

  caveats <<~EOS
    Snapt is not notarized yet. If macOS refuses to open it, run:
      xattr -dr com.apple.quarantine #{appdir}/Snapt.app
  EOS
end
