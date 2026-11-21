{
  description = "Local, per-user screen-time tracker with a static Rofi day timeline";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  };

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f {
        pkgs = import nixpkgs { inherit system; };
      });
    in {
      packages = forAllSystems ({ pkgs }:
        let
          lib = pkgs.lib;
          screen-time = pkgs.rustPlatform.buildRustPackage {
            pname = "screen-time";
            version = "0.1.0";

            src = lib.cleanSourceWith {
              src = lib.cleanSource ./.;
              filter = path: type:
                let name = baseNameOf path; in
                !(lib.elem name [ "target" "dist" ".git" ]);
            };

            cargoLock.lockFile = ./Cargo.lock;
            nativeBuildInputs = [ pkgs.makeWrapper ];
            doCheck = true;
            preCheck = "export TZ=UTC";

            postInstall = ''
              mkdir -p "$out/share/rofi" "$out/systemd"
              install -m 644 share/rofi/screen-time.rasi "$out/share/rofi/screen-time.rasi"
              install -m 644 systemd/screen-time.service "$out/systemd/screen-time.service"
              install -m 755 share/rofi-screen-time "$out/bin/rofi-screen-time"
              wrapProgram "$out/bin/rofi-screen-time" \
                --set SCREEN_TIME_THEME "$out/share/rofi/screen-time.rasi"
              substituteInPlace "$out/systemd/screen-time.service" \
                --replace-fail "ExecStart=%h/.local/bin/screen-time daemon" \
                "ExecStart=$out/bin/screen-time daemon"
            '';

            meta = {
              description = "Local, per-user screen-time tracker with a static Rofi day timeline";
              license = lib.licenses.mit;
              mainProgram = "screen-time";
              platforms = systems;
            };
          };
        in {
          default = screen-time;
          screen-time = screen-time;
        });

      devShells = forAllSystems ({ pkgs }: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            clippy
            rustc
            rustfmt
            shellcheck
          ];
        };
      });

      checks = forAllSystems ({ pkgs }: {
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      });
    };
}
