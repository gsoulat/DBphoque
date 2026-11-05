# Modèle : la CI remplace @URL@ et @SHA256@ (installeur Inno Setup de la Release).
$ErrorActionPreference = 'Stop'

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  fileType       = 'exe'
  url64bit       = '@URL@'
  checksum64     = '@SHA256@'
  checksumType64 = 'sha256'
  # Installation pour tous les utilisateurs : Chocolatey s'exécute en administrateur.
  silentArgs     = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /ALLUSERS'
  validExitCodes = @(0)
}

Install-ChocolateyPackage @packageArgs
