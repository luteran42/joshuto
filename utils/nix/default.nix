{
  lib,
  stdenv,
  rustPlatform,
  toolchain,
  nix-gitignore,
  installShellFiles,
  darwin,
  version ? "git",
}:

rustPlatform.buildRustPackage {
  pname = "joshuto";
  inherit version;

  src = nix-gitignore.gitignoreSource [ ] (lib.cleanSource ../../.);

  cargoLock = {
    lockFile = ../../Cargo.lock;
  };

  buildInputs = [
  ]
  ++ lib.optionals stdenv.isDarwin [
    darwin.apple_sdk.frameworks.SystemConfiguration
    darwin.apple_sdk.frameworks.Foundation
  ];

  nativeBuildInputs = [
    toolchain
    installShellFiles
  ];

  postInstall = ''
    installShellCompletion --cmd joshuto \
      --bash <($out/bin/joshuto completions bash) \
      --zsh <($out/bin/joshuto completions zsh) \
      --fish <($out/bin/joshuto completions fish)
  '';

  meta = with lib; {
    description = "Ranger-like terminal file manager written in Rust";
    homepage = "https://github.com/kamiyaa/joshuto";
    license = licenses.lgpl3Only;
    mainProgram = "joshuto";
  };
}
