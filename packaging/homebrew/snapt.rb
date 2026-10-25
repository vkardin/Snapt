cask "snapt" do
  arch arm: "aarch64", intel: "x64"

  version "2.0.0"
  sha256 arm:   "cedb56d30de0eff7bc24babd1e9135f5ed86abfc32711d9efffe555adcc6af50",
         intel: "cc9dbcc23df2cb897df75e69c49311315b4011dcde061697be677df452bf429f"

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
