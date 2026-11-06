{
  description = "Raptor: native clients for Rex terminal servers";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    # The Ghostty commit libghostty-rs's bindings were generated for (vendor/ghostty).
    ghostty = {
      url = "github:ghostty-org/ghostty/521cf2c5d30bd48028ca61546c64e7913764073d";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, ghostty }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};

      # Zig 0.17 is not in nixpkgs yet.
      zig = pkgs.stdenvNoCC.mkDerivation {
        pname = "zig";
        version = "0.17.1";
        src = pkgs.fetchurl {
          url = "https://ziglang.org/download/0.17.1/zig-x86_64-linux-0.17.1.tar.xz";
          hash = "sha256-NYGGPjy8/Q/ODvxoLOfp6T2pZmipd3kdNWom0qfpsXo=";
        };
        installPhase = ''
          mkdir -p $out/bin
          cp -r zig lib $out/
          ln -s $out/zig $out/bin/zig
        '';
      };

      raptor = pkgs.rustPlatform.buildRustPackage {
        pname = "raptor";
        version = "0.1.0";
        src = pkgs.lib.cleanSource ./.;
        cargoLock = {
          lockFile = ./Cargo.lock;
          outputHashes."libghostty-vt-0.2.1" = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        };
        cargoBuildFlags = [ "-p" "raptor-gtk" ];
        doCheck = false;
        nativeBuildInputs = [ pkgs.pkg-config pkgs.wrapGAppsHook4 zig ];
        buildInputs = [ pkgs.gtk4 pkgs.libadwaita ];
        env = {
          ZIG = "${zig}/bin/zig";
          GHOSTTY_SOURCE_DIR = "${ghostty}";
          GHOSTTY_ZIG_SYSTEM_DIR = pkgs.callPackage "${ghostty}/build.zig.zon.nix" { zig_0_17 = zig; };
        };
        postInstall = ''
          install -Dm644 apps/linux/data/dev.raptor.Raptor.desktop -t $out/share/applications
          install -Dm644 apps/linux/data/dev.raptor.Raptor.svg -t $out/share/icons/hicolor/scalable/apps
          install -Dm644 assets/fonts/*.ttf -t $out/share/raptor/fonts
          install -Dm644 assets/fonts/*.txt assets/fonts/licenses/* -t $out/share/licenses/raptor
        '';
        meta.mainProgram = "raptor";
      };
    in
    {
      packages.${system}.default = raptor;

      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = [ pkgs.pkg-config zig ];
        buildInputs = [ pkgs.gtk4 pkgs.libadwaita ];
        shellHook = ''
          export ZIG=''${ZIG:-${zig}/bin/zig}
          export XDG_DATA_DIRS=${pkgs.adwaita-icon-theme}/share:${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk4}/share/gsettings-schemas/${pkgs.gtk4.name}:$XDG_DATA_DIRS
        '';
      };
    };
}
