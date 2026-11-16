# Pluma

Widgets de desktop para Windows no estilo do Rainmeter, escritos em Rust e feitos para gastar pouca CPU e RAM.

![prévia](preview.png)

## Por que é leve
- Usa a API do Win32 direto (`windows-sys`), sem framework de interface, sem webview e sem runtime de script.
- Cada widget é uma janela *layered* (`UpdateLayeredWindow`) com transparência por pixel.
- O desenho é feito em software com `tiny-skia`, e o texto com `fontdue` usando um cache de glifos.
- As métricas do sistema são coletadas no máximo uma vez por segundo e compartilhadas entre todos os widgets.
- O processo só acorda no `update_ms` de cada widget. Fora isso, fica parado em `GetMessage`.

## Compilar e rodar (no Windows)
```
cargo build --release
target\release\pluma.exe
```
Na primeira execução, ele cria a pasta `widgets\` ao lado do `.exe` com dois exemplos.
Você também pode indicar outra pasta: `pluma.exe C:\meus\widgets`.

- **Arrastar:** clique e arraste o widget. A posição fica salva em `widgets\positions.state`.
- **Ícone na bandeja:** clique esquerdo mostra/oculta os widgets (ocultos, eles não gastam CPU); clique direito abre o menu.
- **Menu** (bandeja ou clique direito num widget): *Mostrar/Ocultar*, *Recarregar*, *Abrir pasta*, *Iniciar com o Windows* e *Sair*.
- **Prévia em PNG** (funciona em qualquer SO): `pluma --preview widgets`

## Formato de um widget (`widgets/*.toml`)
```toml
name = "meu-widget"     # único
x = 100
y = 100
width = 250
height = 100
update_ms = 1000
z = "bottom"            # bottom (no desktop) | normal | top (sempre visível)
opacity = 255
draggable = true
click_through = false
font = "C:/Windows/Fonts/segoeui.ttf"

[background]
color = "#101018C0"     # #RRGGBB ou #RRGGBBAA
radius = 12
border = "#FFFFFF30"

[[element]]
type = "text"           # text | rect | bar | graph
x = 10
y = 10
size = 20
align = "left"
color = "#FFFFFF"
text = "CPU {cpu}% — {time:%H:%M:%S}"
```

| Elemento | Campos |
|---|---|
| `text`  | x, y, text, size, color, align (left/center/right), font |
| `rect`  | x, y, w, h, color, radius |
| `bar`   | x, y, w, h, source (`cpu`/`ram`), color, background, radius |
| `graph` | x, y, w, h, source, color, fill, line_width |

Variáveis de texto: `{cpu}` `{ram}` `{ram_used}` `{ram_total}` `{net_down}` `{net_up}` `{uptime}` `{time:FORMATO}` (formato do [chrono](https://docs.rs/chrono/latest/chrono/format/strftime/)).

## Estrutura
- `src/config.rs`: formato TOML
- `src/metrics.rs`: CPU, RAM, rede, hora e variáveis
- `src/render.rs`: desenho (multiplataforma)
- `src/win.rs`: janelas, arrasto, menu e timers (Win32)
- `src/tray.rs`: ícone da bandeja e início automático

## Próximos passos sugeridos
Recarregar sozinho quando um `.toml` mudar, imagens (PNG), disco/GPU/bateria, comandos ao clicar e plugins.
