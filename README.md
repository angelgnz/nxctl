# nxctl

Alternativa open source a NZXT CAM en Linux, sobre [liquidctl](https://github.com/liquidctl/liquidctl).

- `nxctl`: CLI para estado, perfiles y ajustes manuales.
- `nxctld`: daemon que ajusta ventiladores según temperatura (con histéresis).

## Instalación

Requiere `liquidctl` en el `PATH` y Rust (edición 2024).

```sh
cargo build --release
sudo install -m755 target/release/{nxctl,nxctld} /usr/local/bin/
```

## Uso

```sh
nxctl init                  # crea la config inicial (--force para sobrescribir)
nxctl status [--match kraken] [--json]
nxctl apply [PERFIL]        # por defecto config.default_profile
nxctl set color <MATCH> <CANAL> <MODO> <COLORES...>
nxctl set speed <MATCH> <CANAL> <0-100>
nxctl profiles list
nxctl profiles show <NOMBRE>
```

Flags globales: `--config <RUTA>`, `--dry-run`, `-v/--verbose`.

## Servicio `nxctld`

```sh
nxctld --once --dry-run -v   # prueba: un tick sin cambios reales
sudo mkdir -p /etc/nxctl && sudo cp ~/.config/nxctl/config.toml /etc/nxctl/config.toml
sudo cp contrib/nxctld.service /etc/systemd/system/
sudo systemctl enable --now nxctld
```

Ajusta la ruta de `--config` en la unit según tu instalación. Para unit de usuario,
cópiala a `~/.config/systemd/user/`, quita `--config`, y usa `WantedBy=default.target`.
