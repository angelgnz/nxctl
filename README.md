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
nxctl init-devices [--match kraken]  # ejecuta `liquidctl initialize`
nxctl validate              # valida config.toml sin aplicar cambios
nxctl status [--match kraken] [--json]
nxctl sensors [--json]      # lista sensores de temperatura hwmon (CPU/GPU)
nxctl apply [PERFIL]        # por defecto config.default_profile
nxctl set color <MATCH> <CANAL> <MODO> <COLORES...>
nxctl set speed <MATCH> <CANAL> <0-100>
nxctl profiles list
nxctl profiles show <NOMBRE>
nxctl curves list
nxctl curves show <NOMBRE>
```

Flags globales: `--config <RUTA>`, `--dry-run`, `-v/--verbose`.

### Sensores de temperatura hwmon (CPU/GPU)

Usa `nxctl sensors` para descubrir los sensores térmicos del sistema (`/sys/class/hwmon`):

```sh
nxctl sensors
nxctl sensors --json
```

Para usar un sensor hwmon (como CPU o GPU) en `config.toml`, define `hwmon` (acepta el nombre del chip como `k10temp` o `amdgpu`, o el directorio `hwmon0`) y opcionalmente el `label`:

```toml
# Sensor global para el daemon nxctld:
[daemon.temp_sensor]
hwmon = "k10temp"
label = "Tctl"

# O por regla individual de refrigeración en un perfil:
[[profiles.performance.cooling]]
match_device = "smart"
channel = "fan1"
target = { curve = "aggressive", sensor = { hwmon = "k10temp", label = "Tctl" } }
```

## Servicio `nxctld`

```sh
nxctld --once --dry-run -v               # prueba: un tick sin cambios reales
nxctld --init-devices --once --dry-run   # inicializa dispositivos antes de aplicar perfil
sudo mkdir -p /etc/nxctl && sudo cp ~/.config/nxctl/config.toml /etc/nxctl/config.toml
sudo cp contrib/nxctld.service /etc/systemd/system/
sudo systemctl enable --now nxctld
```

Ajusta la ruta de `--config` en la unit según tu instalación. Para unit de usuario,
cópiala a `~/.config/systemd/user/`, quita `--config`, y usa `WantedBy=default.target`.
