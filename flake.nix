{
  description = "nxctl - Open-source Linux alternative to NZXT CAM built on liquidctl";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      version = cargoToml.workspace.package.version;

      mkNxctlPackage =
        pkgs:
        let
          runtimeLibs = [
            pkgs.libxkbcommon
            pkgs.wayland
            pkgs.xorg.libX11
            pkgs.xorg.libXcursor
            pkgs.xorg.libXi
            pkgs.xorg.libXrandr
            pkgs.xorg.libxcb
            pkgs.libGL
          ];
        in
        pkgs.rustPlatform.buildRustPackage {
          pname = "nxctl";
          inherit version;

          src = pkgs.lib.cleanSource ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = [
            pkgs.pkg-config
            pkgs.makeWrapper
          ];

          buildInputs = runtimeLibs;

          postInstall = ''
            install -Dm644 contrib/72-nzxt-hwmon.rules \
              "$out/lib/udev/rules.d/72-nzxt-hwmon.rules"

            install -Dm644 contrib/nxctl-gui.desktop \
              "$out/share/applications/nxctl-gui.desktop"
            substituteInPlace "$out/share/applications/nxctl-gui.desktop" \
              --replace-fail "/usr/bin/nxctl-gui" "nxctl-gui"

            install -Dm644 contrib/nxctl.svg \
              "$out/share/icons/hicolor/scalable/apps/nxctl.svg"
            install -Dm644 contrib/nxctl-tray.svg \
              "$out/share/icons/hicolor/scalable/status/nxctl-tray.svg"

            install -Dm644 contrib/nxctld.service \
              "$out/lib/systemd/system/nxctld.service"
            substituteInPlace "$out/lib/systemd/system/nxctld.service" \
              --replace-fail "/usr/bin/nxctld" "$out/bin/nxctld"

            install -Dm644 contrib/nxctld-user.service \
              "$out/lib/systemd/user/nxctld.service"
            substituteInPlace "$out/lib/systemd/user/nxctld.service" \
              --replace-fail "/usr/bin/nxctld" "$out/bin/nxctld"

            wrapProgram "$out/bin/nxctl" \
              --prefix PATH : "${pkgs.lib.makeBinPath [ pkgs.liquidctl ]}"

            wrapProgram "$out/bin/nxctld" \
              --prefix PATH : "${pkgs.lib.makeBinPath [ pkgs.liquidctl ]}"

            wrapProgram "$out/bin/nxctl-gui" \
              --prefix PATH : "${pkgs.lib.makeBinPath [ pkgs.liquidctl ]}" \
              --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath runtimeLibs}"
          '';

          meta = with pkgs.lib; {
            description = "Open-source Linux alternative to NZXT CAM over liquidctl (CLI, TUI, daemon, and GUI)";
            homepage = "https://github.com/angelgnz/nxctl";
            license = with licenses; [
              mit
              asl20
            ];
            platforms = platforms.linux;
            mainProgram = "nxctl-gui";
          };
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          nxctlPkg = mkNxctlPackage pkgs;
        in
        {
          default = nxctlPkg;
          nxctl = nxctlPkg;
        }
      );

      apps = forAllSystems (
        system:
        let
          pkg = self.packages.${system}.default;
        in
        {
          default = {
            type = "app";
            program = "${pkg}/bin/nxctl-gui";
          };
          nxctl-gui = {
            type = "app";
            program = "${pkg}/bin/nxctl-gui";
          };
          nxctl = {
            type = "app";
            program = "${pkg}/bin/nxctl";
          };
          nxctld = {
            type = "app";
            program = "${pkg}/bin/nxctld";
          };
        }
      );

      nixosModules = {
        default = self.nixosModules.nxctl;
        nxctl =
          {
            config,
            lib,
            pkgs,
            ...
          }:
          let
            cfg = config.services.nxctl;
          in
          {
            options.services.nxctl = {
              enable = lib.mkEnableOption "NXCTL thermal and RGB control daemon and utilities";

              package = lib.mkOption {
                type = lib.types.package;
                default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
                defaultText = lib.literalExpression "self.packages.\${pkgs.stdenv.hostPlatform.system}.default";
                description = "The nxctl package to install.";
              };

              userService = lib.mkOption {
                type = lib.types.bool;
                default = true;
                description = "Enable the rootless nxctld systemd user service.";
              };
            };

            config = lib.mkIf cfg.enable {
              environment.systemPackages = [
                cfg.package
                pkgs.liquidctl
              ];

              services.udev.packages = [ cfg.package ];

              systemd.user.services.nxctld = lib.mkIf cfg.userService {
                description = "nxctl thermal and RGB control daemon";
                after = [ "default.target" ];
                wantedBy = [ "default.target" ];
                path = [ pkgs.liquidctl ];
                serviceConfig = {
                  ExecStart = "${cfg.package}/bin/nxctld";
                  ExecReload = "${pkgs.coreutils}/bin/kill -HUP $MAINPID";
                  Restart = "on-failure";
                  RestartSec = 5;
                };
              };
            };
          };
      };
    };
}
