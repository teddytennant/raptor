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

      zig = pkgs.zig_0_17;

      raptor = pkgs.rustPlatform.buildRustPackage {
        pname = "raptor";
        version = "0.1.0";
        src = pkgs.lib.cleanSource ./.;
        cargoLock = {
          lockFile = ./Cargo.lock;
          outputHashes."libghostty-vt-0.2.1" = "sha256-oy/S2Ir+ltndFcvmMiCyZ5j07w91lKdNxACUJr7I4EU=";
        };
        cargoBuildFlags = [ "-p" "raptor-gtk" ];
        doCheck = false;
        nativeBuildInputs = [ pkgs.pkg-config pkgs.wrapGAppsHook4 ];
        buildInputs = [ pkgs.gtk4 pkgs.libadwaita ];
        # Icons like tab-new come from the Adwaita theme, which isn't there outside GNOME.
        preFixup = ''
          gappsWrapperArgs+=(--prefix XDG_DATA_DIRS : ${pkgs.adwaita-icon-theme}/share)
        '';
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
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [ pkgs.gtk4 pkgs.libadwaita ];
        shellHook = ''
          export ZIG=''${ZIG:-${zig}/bin/zig}
          export XDG_DATA_DIRS=${pkgs.adwaita-icon-theme}/share:${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk4}/share/gsettings-schemas/${pkgs.gtk4.name}:$XDG_DATA_DIRS
        '';
      };
    };
}
