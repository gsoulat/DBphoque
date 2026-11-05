# Distribution de DBphoque

À chaque Release, la CI (`.github/workflows/release.yml`) attache les paquets
d'installation à la Release GitHub, puis les publie sur les gestionnaires de
paquets. Chaque canal a besoin d'un secret du dépôt (Settings → Secrets and
variables → Actions). Sans son secret, le canal est ignoré et la Release
réussit quand même.

| Canal | Commande pour l'utilisateur | Secret |
|---|---|---|
| Release GitHub | téléchargement direct | aucun |
| APT (Debian, Ubuntu) | `sudo apt install dbphoque` | `APT_GPG_PRIVATE_KEY` |
| Homebrew (macOS) | `brew install --cask gsoulat/tap/dbphoque` | `HOMEBREW_TAP_TOKEN` |
| winget (Windows) | `winget install gsoulat.DBphoque` | `WINGET_TOKEN` |
| Chocolatey (Windows) | `choco install dbphoque` | `CHOCO_API_KEY` |

## Paquets attachés à chaque Release

| Système | Fichiers |
|---|---|
| macOS | `.dmg` et `.zip` (Apple Silicon, Intel) |
| Linux | `.deb`, `.AppImage`, `.tar.gz` (x86_64) |
| Windows | `-setup.exe` (Inno Setup) et `.zip` portable (x86_64) |

## APT : dépôt signé sur GitHub Pages

Le job `apt` publie un dépôt APT dans la branche `gh-pages`, sous `apt/`.

1. Créer une clé GPG dédiée, sans phrase de passe (la CI signe sans interaction) :

   ```bash
   gpg --batch --passphrase '' --quick-gen-key "DBphoque APT <ton-email>" ed25519 sign never
   ```

2. Exporter la clé privée dans le secret `APT_GPG_PRIVATE_KEY` :

   ```bash
   gpg --armor --export-secret-keys "DBphoque APT" | gh secret set APT_GPG_PRIVATE_KEY --repo gsoulat/DBphoque
   ```

3. Après la première Release, activer GitHub Pages : Settings → Pages → Deploy
   from a branch → `gh-pages`, dossier `/ (root)`.

Installation pour l'utilisateur :

```bash
curl -fsSL https://gsoulat.github.io/DBphoque/apt/dbphoque.gpg | sudo tee /usr/share/keyrings/dbphoque.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/dbphoque.gpg] https://gsoulat.github.io/DBphoque/apt stable main" | sudo tee /etc/apt/sources.list.d/dbphoque.list
sudo apt update && sudo apt install dbphoque
```

Seule la dernière version est servie, pour que la branche `gh-pages` reste légère.

## Homebrew : tap personnel

1. Créer le dépôt public `gsoulat/homebrew-tap` (le préfixe `homebrew-` est obligatoire).
2. Créer un jeton à grain fin limité à ce dépôt, avec la permission
   *Contents: Read and write*, et l'enregistrer dans `HOMEBREW_TAP_TOKEN`.

Le job écrit `Casks/dbphoque.rb` à partir de `packaging/homebrew/dbphoque.rb`.
L'application n'étant pas notariée, le cask retire l'attribut de quarantaine
après l'installation.

## winget

1. Créer un jeton classique avec la permission `public_repo` et l'enregistrer
   dans `WINGET_TOKEN`. `wingetcreate` crée un fork de `microsoft/winget-pkgs`
   sur ton compte et y ouvre une pull request.
2. La **première** soumission est relue par l'équipe winget (quelques jours) ;
   les suivantes sont en général fusionnées automatiquement.

Les modèles sont dans `packaging/winget/`. Le champ `License` y vaut
« Tous droits réservés » tant que le dépôt n'a pas de fichier LICENSE.

## Chocolatey

1. Créer un compte sur <https://community.chocolatey.org>, puis copier la clé
   d'API (page du compte) dans `CHOCO_API_KEY`.
2. Chaque version passe par la modération de Chocolatey (validation
   automatique, puis humaine pour les premières versions).

Les modèles sont dans `packaging/chocolatey/` : le paquet télécharge
l'installeur de la Release, vérifie son SHA-256 et l'installe en silencieux.

## Limites connues

- Les exécutables ne sont pas signés : Gatekeeper (macOS) et SmartScreen
  (Windows) demandent une confirmation. Une signature Apple Developer
  (99 $/an) et un certificat de signature de code Windows les feraient
  disparaître.
- Pas de build Linux ARM (`aarch64`).
