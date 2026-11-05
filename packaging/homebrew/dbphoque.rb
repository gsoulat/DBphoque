# Modèle du cask Homebrew : la CI remplace @VERSION@, @SHA256_ARM@ et
# @SHA256_INTEL@ puis le publie dans gsoulat/homebrew-tap (Casks/dbphoque.rb).
cask "dbphoque" do
  arch arm: "apple-silicon", intel: "intel"

  version "@VERSION@"
  sha256 arm:   "@SHA256_ARM@",
         intel: "@SHA256_INTEL@"

  url "https://github.com/gsoulat/DBphoque/releases/download/v#{version}/DBphoque-v#{version}-macos-#{arch}.dmg"
  name "DBphoque"
  desc "Client de bases de données léger : SQLite, PostgreSQL, MySQL, BigQuery"
  homepage "https://github.com/gsoulat/DBphoque"

  livecheck do
    url :url
    strategy :github_latest
  end

  app "DBphoque.app"

  # L'application n'est pas notariée par Apple : sans cela, Gatekeeper
  # refuse de l'ouvrir au premier lancement.
  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-dr", "com.apple.quarantine", "#{appdir}/DBphoque.app"]
  end

  zap trash: "~/Library/Application Support/fr.gsoulat.DBphoque"
end
