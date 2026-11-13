# OpenNX

Alternativa open source a NZXT CAM en Linux, sobre [liquidctl](https://github.com/liquidctl/liquidctl).

- `nxctl`: CLI para estado, perfiles y ajustes manuales.
- `opennxd`: daemon que ajusta ventiladores según temperatura (con histéresis).

## Instalación

Requiere `liquidctl` en el `PATH` y Rust (edición 2024).

```sh
cargo build --release
sudo install -m755 target/release/{nxctl,opennxd} /usr/local/bin/
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

## Servicio `opennxd`

```sh
opennxd --once --dry-run -v   # prueba: un tick sin cambios reales
sudo mkdir -p /etc/opennx && sudo cp ~/.config/opennx/config.toml /etc/opennx/config.toml
sudo cp contrib/opennxd.service /etc/systemd/system/
sudo systemctl enable --now opennxd
```

Ajusta la ruta de `--config` en la unit según tu instalación. Para unit de usuario,
cópiala a `~/.config/systemd/user/`, quita `--config`, y usa `WantedBy=default.target`.
