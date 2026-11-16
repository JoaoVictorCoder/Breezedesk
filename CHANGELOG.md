# Changelog

Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/), com [versionamento semântico](https://semver.org/lang/pt-BR/).

## [0.2.0] - 2026-11-16
### Adicionado
- Ícone na bandeja do sistema: o clique esquerdo mostra/oculta os widgets e o clique direito abre o menu.
- Opções *Mostrar/Ocultar widgets* e *Iniciar com o Windows* no menu.
- O ícone volta sozinho se o Explorer reiniciar.

### Alterado
- O projeto passou a se chamar **Pluma** (antes RustMeter).
- Widgets ocultos não são redesenhados (economia de CPU).

## [0.1.0] - 2026-11-16
### Adicionado
- Widgets em janelas transparentes (UpdateLayeredWindow), configurados por arquivos TOML.
- Elementos `text`, `rect`, `bar` e `graph`; variáveis de CPU, RAM, rede, tempo ligado e hora.
- Arrastar widgets com o mouse, com a posição salva.
- Menu de clique direito: Recarregar, Abrir pasta e Sair.
- Modo `--preview`, que gera PNGs dos widgets.
