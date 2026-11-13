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
nxctl sensors [--json]      # lista sensores de temperatura hwmon (CPU/GPU) y NVIDIA (nvidia-smi)
nxctl apply [PERFIL]        # por defecto config.default_profile
nxctl set color <MATCH> <CANAL> <MODO> <COLORES...>
nxctl set speed <MATCH> <CANAL> <0-100>
nxctl profiles list
nxctl profiles show <NOMBRE>
nxctl curves list
nxctl curves show <NOMBRE>
nxctl daemon status [--socket RUTA] [--json]
nxctl daemon reload [--socket RUTA]
nxctl daemon profile <NOMBRE> [--socket RUTA]
```

Flags globales: `--config <RUTA>`, `--dry-run`, `-v/--verbose`.

### Sensores de temperatura hwmon (CPU/GPU) y NVIDIA (`nvidia-smi`)

Usa `nxctl sensors` para descubrir los sensores térmicos del sistema (`/sys/class/hwmon`) y tarjetas gráficas NVIDIA (vía `nvidia-smi`):

```sh
nxctl sensors
nxctl sensors --json
```

Para usar un sensor hwmon (como CPU o GPU AMD/Intel) en `config.toml`, define `hwmon` (acepta el nombre del chip como `k10temp` o `amdgpu`, o el directorio `hwmon0`) y opcionalmente el `label`:

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

Para usar una GPU NVIDIA como sensor de temperatura (requiere `nvidia-smi`), define `nvidia = true` y opcionalmente el índice `gpu` (por defecto `0` si se omite):

```toml
# Sensor global para el daemon nxctld:
[daemon.temp_sensor]
nvidia = true
gpu = 0

# O por regla individual de refrigeración en un perfil:
[[profiles.performance.cooling]]
match_device = "smart"
channel = "fan2"
target = { curve = "aggressive", sensor = { nvidia = true, gpu = 0 } }
```

## Permisos rootless (`udev`)

Los drivers del kernel (`nzxt_kraken3`, `nzxt-smart2`) exponen las velocidades por `hwmon` (`pwm*`). Para controlar ventiladores/bomba sin `sudo`, instala la regla `udev`:

```sh
sudo cp contrib/72-nzxt-hwmon.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger -s hwmon
```

## Servicio `nxctld`

### Opción recomendada (servicio de usuario, sin `sudo`)

```sh
nxctld --once --dry-run -v               # prueba: un tick sin cambios reales
nxctld --init-devices --once --dry-run   # inicializa dispositivos antes de aplicar perfil
mkdir -p ~/.config/systemd/user
cp contrib/nxctld-user.service ~/.config/systemd/user/nxctld.service
systemctl --user enable --now nxctld
systemctl --user reload nxctld           # recarga ~/.config/nxctl/config.toml vía SIGHUP
```

### Opción de sistema

```sh
sudo mkdir -p /etc/nxctl && sudo cp ~/.config/nxctl/config.toml /etc/nxctl/config.toml
sudo cp contrib/nxctld.service /etc/systemd/system/
sudo systemctl enable --now nxctld
sudo systemctl reload nxctld             # recarga config.toml vía SIGHUP
```

También puedes recargar o cambiar de perfil en caliente vía IPC (socket Unix):

```sh
nxctl daemon status
nxctl daemon reload
nxctl daemon profile performance
```

Ajusta la ruta de `--config` en la unit de sistema según tu instalación.
