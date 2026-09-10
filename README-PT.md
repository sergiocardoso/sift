# Sift

<p align="center">
  <img
    src="https://supabase.flokin.com.br/storage/v1/object/public/projects/sift/fefbec4b-ddda-425f-a9f1-8581b4a8f19b.png"
    alt="Sift"
    width="220"
  />
</p>

<p align="center">
  <strong>Organização segura e local do sistema de arquivos.</strong>
</p>

<p align="center">
  Organize, limpe, monitore e entenda suas pastas sem abrir mão do controle.
</p>

**Um CLI local para organizar pastas bagunçadas com segurança, previsibilidade e de acordo com sua vontade.**

![License: Apache 2.0](https://img.shields.io/badge/license-Apache%202.0-blue.svg)
![Rust](https://img.shields.io/badge/rust-stable-orange.svg)
![Version](https://img.shields.io/badge/version-0.1.0-lightgrey.svg)

O Sift ajuda a transformar pastas como `Downloads`, `Desktop`, diretórios compartilhados, pastas de ativos 3D e espaços de trabalho temporários em algo que você consiga entender novamente.

Ele faz isso sem depender de serviços em nuvem ou IA, e sem mover arquivos silenciosamente às suas costas.

```text
pasta bagunçada
    ↓
varredura
    ↓
classificação
    ↓
plano
    ↓
revisão
    ↓
--apply
    ↓
sistema de arquivos organizado + histórico
```

O comportamento padrão é deliberadamente conservador:

```text
sem --apply
→ sem mutação no sistema de arquivos
```

---

## Por que o Sift?

As pastas acumulam entropia.

Um diretório de downloads começa com alguns PDFs e capturas de tela e, eventualmente, vira uma mistura de documentos, arquivos compactados, vídeos, arquivos de código-fonte, exportações, conjuntos de dados, modelos 3D, arquivos temporários e pastas de projetos esquecidos.

As soluções usuais têm trade-offs:

- organização manual não escala;
- scripts shell pontuais são fáceis de esquecer e arriscados de reutilizar;
- automação pode ficar perigosa quando sobrescreve ou apaga arquivos;
- organizadores baseados em IA podem ser difíceis de prever, auditar ou executar totalmente localmente.

O Sift segue uma abordagem diferente.

### Previsível

A classificação é determinística e baseada em nomes de arquivo, extensões, metadados e regras explícitas.

A mesma entrada produz o mesmo plano.

### Seguro por padrão

O Sift antecipa as mudanças primeiro. Comandos que alteram arquivos exigem autorização explícita com `--apply`.

### Local-first

O Sift trabalha no seu sistema de arquivos. Ele não precisa enviar nomes de arquivo, conteúdo ou metadados para um serviço externo para executar sua tarefa principal.

### Explicável

Um plano do Sift diz o que vai acontecer antes que aconteça:

```text
invoice.pdf     → Documents/
photo.jpg       → Images/
model.stl       → 3D/
data.json       → Data/
```

### Conservador com dados importantes

Projetos de software, entradas ocultas, links simbólicos, colisões e limites de travessia protegidos são deixados de lado intencionalmente.

### Auditável

Operações executadas são registradas em histórico.

Movimentações bem-sucedidas de arquivos podem ser revertidas com `sift undo` quando o sistema de arquivos vivo ainda torna a reversão segura.

### Automatizável

Quando estiver pronto, o **Sift Watch** pode transformar uma pasta em uma caixa de entrada continuamente organizada, mantendo as mesmas regras de segurança da organização manual.

---

## O que o Sift é — e o que ele não é

O Sift é um CLI de organização e manutenção do sistema de arquivos.

Ele foi projetado para:

- inspecionar diretórios;
- organizar arquivos soltos por tipo;
- organizar pastas aninhadas elegíveis no lugar com modo recursivo;
- identificar lixo de alta confiança;
- diagnosticar entradas do sistema de arquivos potencialmente interessantes;
- aplicar regras locais personalizadas;
- manter histórico de operações;
- desfazer movimentações bem-sucedidas quando seguro;
- organizar continuamente novos arquivos com o Watch.

O Sift **não é**:

- um serviço de armazenamento em nuvem;
- uma ferramenta de sincronização de arquivos;
- um mecanismo de busca por indexação de conteúdo;
- um classificador por IA;
- um removedor de arquivos duplicados;
- uma ferramenta de exclusão recursiva;
- uma ferramenta que sobrescreve silenciosamente destinos existentes.

---

## Exemplo em 30 segundos

Imagine este diretório:

```text
Downloads/
├── invoice.pdf
├── vacation.jpg
├── backup.zip
├── data.json
├── model.stl
├── experiment.rs
├── notes.xyz
└── my-project/
    ├── Cargo.toml
    └── src/
```

Pré-visualize o plano:

```bash
sift organize ~/Downloads
```

O Sift pode planejar algo assim:

```text
invoice.pdf      → Documents/
vacation.jpg     → Images/
backup.zip       → Archives/
data.json         → Data/
model.stl         → 3D/
experiment.rs     → Code/
notes.xyz         → Other/
my-project/       → protegido: projeto de software
```

Nada mudou ainda.

Aplique somente depois de revisar:

```bash
sift organize ~/Downloads --apply
```

Em seguida, inspecione o histórico de operações:

```bash
sift history
```

E, se necessário, reverta movimentações bem-sucedidas:

```bash
sift undo hist-<operation-id>
```

---

# Instalação

O Sift é escrito em Rust.

## Script de instalação

```bash
curl -fsSL https://raw.githubusercontent.com/sergiocardoso/sift/main/install.sh | sh
```

Baixa a última release pré-compilada pro seu SO/arquitetura (Linux e macOS, x86_64/aarch64), verifica o checksum SHA256, e instala em `~/.local/bin/sift` — nunca com `sudo`. Também checa se `ffprobe` está no seu `PATH`; se não estiver, só **informa** o comando certo pro seu sistema, e oferece rodar esse comando só em terminal interativo, só depois de você confirmar que sim.

`ffprobe` é totalmente opcional. A estratégia `video` do Sift funciona sem ele (só containers MP4/MOV) e passa a usá-lo automaticamente pra mais formatos e metadados mais ricos (`{duration}`, `{fps}`) quando ele está instalado — veja "Estratégias de organização baseadas em metadados" mais abaixo.

## Compilar a partir do código-fonte

```bash
git clone https://github.com/sergiocardoso/sift.git
cd sift
cargo build --release
```

O binário ficará disponível em:

```text
target/release/sift
```

Execute diretamente:

```bash
./target/release/sift --help
```

Ou instale a compilação local no diretório de binários do Cargo:

```bash
cargo install --path .
```

Depois:

```bash
sift --help
```

## Opcional: `sift-tray` (UI de bandeja/menu bar)

Um app pequeno e totalmente separado que mostra um ícone perto do relógio (menu bar no macOS, bandeja no Windows/Linux) listando suas pastas monitoradas — nome, estado (rodando/pausado/parado/com erro de config), com "Abrir pasta" e "Pausar"/"Retomar" por watch. É uma camada de UI fina sobre a mesma biblioteca `sift` que o CLI usa (`watch::registry::list`, as mesmas funções `cmd_watch_pause`/`cmd_watch_resume` que `sift watch pause`/`resume` chamam) — nunca fala com o daemon do watch diretamente e nunca reimplementa lógica de watch.

Vive no seu próprio pacote de workspace especificamente pra que instalar/compilar o CLI `sift` nunca puxe dependências gráficas (GTK no Linux etc.).

Cada [Release do GitHub](https://github.com/sergiocardoso/sift/releases) já inclui um arquivo `sift-tray` pré-compilado pra Linux (x86_64) e macOS (x86_64/aarch64) ao lado do próprio `sift` — baixe e extraia ao lado do seu `sift` já instalado (ex: `~/.local/bin`), do mesmo jeito que qualquer outro arquivo de release. O `install.sh` ainda não baixa isso automaticamente, então por enquanto é um passo manual. Ainda não tem arquivo pré-compilado pra Linux aarch64 (veja [Contributing](#contributing) se quiser ajudar com cross-compile de GTK pra esse alvo), e não tem build pra Windows nenhuma — veja [Suporte de plataforma](#suporte-de-plataforma).

Ou compile você mesmo a partir do código-fonte:

```bash
cargo build -p sift-tray --release
./target/release/sift-tray
```

No Linux isso precisa de GTK3, uma implementação de AppIndicator, e `libxdo` (usado pelo seletor de pasta nativo) disponíveis na hora de compilar — ex. no Debian/Ubuntu: `sudo apt-get install libgtk-3-dev libayatana-appindicator3-dev libxdo-dev` (distros mais antigas: `libappindicator3-dev` em vez do pacote `ayatana`). Ainda não tem autostart/empacotamento (sem `.app` no macOS, sem entrada de inicialização no Windows, sem autostart via `.desktop` no Linux) — por enquanto é só "rode o binário".

### Lançado automaticamente pelo `sift watch start`, best-effort

Depois que o `sift-tray` estiver instalado (baixado de uma release ou compilado a partir do código-fonte, conforme acima) e ele estiver ao lado do `sift` no mesmo diretório, ou em qualquer lugar do `PATH`, `sift watch start`/`sift watch resume` tentam lançá-lo automaticamente, destacado, em background — sem precisar de um passo separado depois disso.

Isso é inteiramente best-effort e silencioso em qualquer caso: se o binário `sift-tray` não estiver instalado (o caso comum, já que ainda é um download manual mesmo agora que está na release), se não houver servidor de display (um servidor headless, um container, uma execução de CI), ou se já houver uma instância do `sift-tray` rodando, o `watch start` continua funcionando normalmente e nunca imprime um aviso sobre isso. Só existe uma instância do `sift-tray` rodando por vez (um lock de singleton, mesmo mecanismo do próprio daemon de watch), então iniciar vários watches em sequência nunca abre vários ícones de bandeja.

---

# Início rápido

```bash
# Pré-visualizar a organização do diretório atual
sift

# Pré-visualizar outro diretório
sift ~/Downloads

# Inspecionar entradas sem planejar mutações
sift scan ~/Downloads

# Pré-visualizar organização
sift organize ~/Downloads

# Aplicar o plano de organização
sift organize ~/Downloads --apply

# Inspecionar diretórios aninhados também
sift organize ~/Downloads --recursive

# Diagnosticar um diretório
sift doctor ~/Downloads

# Pré-visualizar limpeza de lixo
sift clean ~/Downloads

# Enviar candidatos de lixo aprovados para a lixeira do sistema
sift clean ~/Downloads --apply

# Mostrar histórico de operações
sift history
```

---

# Visão geral dos comandos

| Comando | Finalidade | Modifica? |
|---|---|---:|
| `sift [path]` | Pré-visualização segura de organização | Não |
| `sift scan [path]` | Inspecionar entradas do diretório | Não |
| `sift organize [path]` | Construir um plano de organização | Somente com `--apply` |
| `sift clean [path]` | Construir um plano de limpeza | Somente com `--apply` |
| `sift doctor [path]` | Relatar achados do sistema de arquivos | Não |
| `sift history` | Mostrar operações registradas | Não |
| `sift undo <id>` | Reverter movimentações gravadas com sucesso | Sim |
| `sift init [path]` | Criar um `.sift.toml` inicial | Sim |
| `sift watch ...` | Gerenciar organização contínua | A execução do watch é explicitamente autorizada com `--auto-apply` |

---

# Referência completa da CLI

Esta seção documenta a superfície atual da CLI em detalhes.

## Comandos globais

```bash
sift --help
sift --version
```

### Invocação direta

```bash
sift [PATH]
```

`PATH` padrão é o diretório atual (`.`).

O comando sem subcomando equivale a uma pré-visualização segura e não recursiva de organização.

Exemplos:

```bash
sift
sift .
sift ~/Downloads
```

Ele nunca aplica alterações automaticamente.

---

## `sift scan`

Inspeciona entradas do sistema de arquivos sem alterar nada.

```bash
sift scan [PATH] [--json] [--recursive]
```

`PATH` padrão é `.`.

### Parâmetros

| Parâmetro | Obrigatório | Descrição |
|---|---:|---|
| `[PATH]` | Não | Diretório a inspecionar. Padrão: `.` |
| `--json` | Não | Exibe JSON legível para máquina em vez de saída humana |
| `--recursive` | Não | Desce para subdiretórios elegíveis |

### Exemplos

```bash
sift scan
sift scan ~/Downloads
sift scan ~/Downloads --recursive
sift scan ~/Downloads --json
sift scan ~/Downloads --recursive --json
```

A varredura recursiva não desce por limites de travessia protegidos, como diretórios ocultos, links simbólicos, projetos de software, diretórios de saída de build ou os próprios diretórios de categoria do Sift.

---

## `sift organize`

Classifica arquivos soltos e cria um plano para organizá-los em diretórios de categoria.

```bash
sift organize [PATH] [--apply] [--json] [--verbose] [--recursive]
```

`PATH` padrão é `.`.

### Parâmetros

| Parâmetro | Obrigatório | Descrição |
|---|---:|---|
| `[PATH]` | Não | Diretório a organizar. Padrão: `.` |
| `--apply` | Não | Executa o plano. Sem isso, o Sift apenas antecipa |
| `--json` | Não | Exibe o plano em JSON |
| `--verbose` | Não | Mostra entradas ignoradas individualmente em vez de agrupar grandes conjuntos |
| `--recursive` | Não | Organiza cada diretório aninhado elegível em seu próprio contexto local |

### Exemplos

Pré-visualização:

```bash
sift organize ~/Downloads
```

Aplicar:

```bash
sift organize ~/Downloads --apply
```

Inspecionar o plano completo:

```bash
sift organize ~/Downloads --verbose
```

Usar em script:

```bash
sift organize ~/Downloads --json
```

Organizar diretórios aninhados no lugar:

```bash
sift organize ~/Downloads --recursive
```

Aplicar organização recursiva:

```bash
sift organize ~/Downloads --recursive --apply
```

### Comportamento importante

O modo recursivo preserva o contexto do diretório.

Dado:

```text
Downloads/
├── invoice.pdf
└── Client A/
    ├── proposal.pdf
    └── logo.png
```

Executando:

```bash
sift organize Downloads --recursive --apply
```

produz o equivalente a:

```text
Downloads/
├── Documents/
│   └── invoice.pdf
└── Client A/
    ├── Documents/
    │   └── proposal.pdf
    └── Images/
        └── logo.png
```

O Sift não achata `Client A/proposal.pdf` para `Downloads/Documents/`.

### Lixo durante a organização

Extensões de lixo com alta confiança reconhecidas atualmente pelo Sift:

```text
.tmp
.swp
.swo
```

Essas podem aparecer como ações de **Trash** em um plano de organização em vez de serem movidas para um diretório de categoria.

Nada é enviado para a lixeira até que `--apply` seja usado explicitamente.

---

## `sift clean`

Encontra candidatos de limpeza com alta confiança e os envia para a lixeira do sistema operacional quando aplicados explicitamente.

```bash
sift clean [PATH] [--apply] [--json] [--verbose]
```

`PATH` padrão é `.`.

### Parâmetros

| Parâmetro | Obrigatório | Descrição |
|---|---:|---|
| `[PATH]` | Não | Diretório a limpar. Padrão: `.` |
| `--apply` | Não | Envia os candidatos planejados para a lixeira do sistema |
| `--json` | Não | Exibe o plano de limpeza em JSON |
| `--verbose` | Não | Mostra cada entrada ignorada individualmente |

### Exemplos

Pré-visualização:

```bash
sift clean ~/Downloads
```

Aplicar:

```bash
sift clean ~/Downloads --apply
```

Pré-visualização verbosa:

```bash
sift clean ~/Downloads --verbose
```

JSON:

```bash
sift clean ~/Downloads --json
```

### Por que não existe `clean --recursive`

A limpeza recursiva foi intencionalmente deixada fora da CLI atual.

A limpeza tem um risco destrutivo maior do que a organização, por isso o Sift mantém essa superfície deliberadamente mais estreita.

### Candidatos de limpeza internos

O conjunto interno de lixo de alta confiança atualmente é:

```text
*.tmp
*.swp
*.swo
```

Outros arquivos não são automaticamente tratados como lixo só porque parecem antigos ou desnecessários.

> Operações de lixeira não são restauradas atualmente por `sift undo`. `undo` serve para movimentações de arquivo registradas com sucesso. A lixeira do sistema operacional continua responsável pela recuperação da lixeira.

---

## `sift doctor`

Inspeciona um diretório em busca de entradas do sistema de arquivos potencialmente interessantes ou arriscadas.

`doctor` é somente de relatório e nunca lê o conteúdo dos arquivos para tomar essas decisões.

```bash
sift doctor [PATH] [--json] [--recursive]
```

`PATH` padrão é `.`.

### Parâmetros

| Parâmetro | Obrigatório | Descrição |
|---|---:|---|
| `[PATH]` | Não | Diretório a inspecionar. Padrão: `.` |
| `--json` | Não | Exibe os achados em JSON |
| `--recursive` | Não | Inspeciona diretórios aninhados elegíveis usando os mesmos limites de travessia da organização recursiva |

### Exemplos

```bash
sift doctor ~/Downloads
sift doctor ~/Downloads --recursive
sift doctor ~/Downloads --json
```

### Achados atuais

`doctor` pode relatar:

- projetos de software;
- diretórios protegidos;
- links simbólicos;
- entradas ocultas;
- nomes de arquivos com aparência sensível;
- arquivos maiores que 100 MB;
- arquivos compactados com mais de um ano;
- diretórios conhecidos de saída de build/dependências.

Os nomes atuais de diretórios de build/dependência incluem:

```text
node_modules
target
.venv
```

A verificação de nomes sensíveis é baseada no nome do arquivo. O Sift não precisa abrir o arquivo para sinalizá-lo.

---

## `sift history`

Lista as operações que o Sift registrou.

```bash
sift history
```

Exemplo:

```bash
sift history
```

O histórico torna a organização aplicada auditável e fornece os IDs de operação usados pelo `sift undo`.

---

## `sift undo`

Reverte movimentações bem-sucedidas de arquivos de uma operação registrada.

```bash
sift undo <OPERATION_ID>
```

### Parâmetros

| Parâmetro | Obrigatório | Descrição |
|---|---:|---|
| `<OPERATION_ID>` | Sim | ID do histórico exibido por `sift history`, como `hist-...` |

Exemplo:

```bash
sift undo hist-1699999999999999999
```

### Segurança do undo

O undo não move arquivos de volta sem pensar.

Imediatamente antes de restaurar um arquivo, o Sift verifica que:

- o local original ainda está livre;
- o destino movido ainda existe;
- o destino é um arquivo regular;
- o destino não se tornou um link simbólico;
- o destino não se tornou um diretório.

Se essas premissas não forem mais verdadeiras, o Sift recusa esse undo em vez de sobrescrever ou mover algo inseguro.

Uma entrada separada no histórico registra a tentativa de undo.

### O que o undo não restaura

Ações de lixeira não são restauradas pelo sistema de histórico do Sift.

Se um arquivo foi enviado para a lixeira do sistema operacional, a recuperação pertence à implementação da lixeira do seu sistema.

---

## `sift init`

Cria um arquivo de configuração inicial `.sift.toml`.

```bash
sift init [PATH] [--force]
```

`PATH` padrão é `.`.

### Parâmetros

| Parâmetro | Obrigatório | Descrição |
|---|---:|---|
| `[PATH]` | Não | Diretório em que `.sift.toml` deve ser criado. Padrão: `.` |
| `--force` | Não | Sobrescreve um `.sift.toml` existente |

Exemplos:

```bash
sift init
sift init ~/Downloads
sift init ~/Downloads --force
```

Sem `--force`, o Sift recusa sobrescrever um `.sift.toml` existente.

---

# Sift Watch

O Sift Watch transforma pastas selecionadas em caixas de entrada continuamente organizadas.

A distinção importante é que a mutação automática requer uma autorização explícita e persistente:

```bash
--auto-apply
```

Registrar um watch **não** começa a monitorar imediatamente.

Um watch recém-registrado começa no estado `stopped`.

```text
add
 ↓
stopped
 ↓ start
running
 ↕
pause / resume
 ↓ stop
stopped
```

Arquivos já existentes não são preenchidos automaticamente quando o watch começa.

---

## `sift watch add`

Registra um diretório para organização automática.

```bash
sift watch add <PATH> --auto-apply [--recursive]
```

### Parâmetros

| Parâmetro | Obrigatório | Descrição |
|---|---:|---|
| `<PATH>` | Sim | Diretório existente para registrar |
| `--auto-apply` | **Sim** | Autorização persistente explícita que permite ao Watch organizar automaticamente novos arquivos elegíveis |
| `--recursive` | Não | Também monitora caminhos aninhados elegíveis usando os limites da organização recursiva |

Exemplos:

```bash
sift watch add ~/Downloads --auto-apply
sift watch add ~/Desktop/Inbox --auto-apply --recursive
```

Isso apenas registra o watch.

Inicie separadamente:

```bash
sift watch start ~/Downloads
```

---

## `sift watch list`

Lista os watches registrados.

```bash
sift watch list [--json]
```

### Parâmetros

| Parâmetro | Obrigatório | Descrição |
|---|---:|---|
| `--json` | Não | Exibe as informações do registro em JSON |

Exemplos:

```bash
sift watch list
sift watch list --json
```

Os registros do watch incluem estados operacionais úteis, como:

- caminho;
- estado;
- modo recursivo;
- autorização de auto-aplicação;
- quantidade de arquivos organizados;
- contagem de erros;
- última atividade;
- último erro.

---

## `sift watch status`

Mostra o estado do watch.

```bash
sift watch status [PATH] [--json]
```

### Parâmetros

| Parâmetro | Obrigatório | Descrição |
|---|---:|---|
| `[PATH]` | Não | Mostra um watch registrado. Se omitido, mostra o estado geral ou informações da lista |
| `--json` | Não | Exibe informações do watch em JSON |

Exemplos:

```bash
sift watch status
sift watch status ~/Downloads
sift watch status ~/Downloads --json
```

---

## `sift watch start`

Inicia um watch parado e registrado.

```bash
sift watch start <PATH>
```

Exemplo:

```bash
sift watch start ~/Downloads
```

Iniciar um watch não organiza arquivos que já estavam presentes antes do watch entrar em atividade.

Se você quiser organizar arquivos existentes primeiro:

```bash
sift organize ~/Downloads --apply
sift watch start ~/Downloads
```

---

## `sift watch pause`

Pausa um watch em execução mantendo o registro.

```bash
sift watch pause <PATH>
```

Exemplo:

```bash
sift watch pause ~/Downloads
```

Candidatos pendentes que ainda não se tornaram estáveis são descartados em vez de ficarem em fila para uma recuperação posterior.

---

## `sift watch resume`

Retoma um watch pausado.

```bash
sift watch resume <PATH>
```

Exemplo:

```bash
sift watch resume ~/Downloads
```

A retomada processa novos eventos a partir deste ponto. Ela não retroalimenta arquivos criados enquanto o watch estava pausado.

---

## `sift watch stop`

Interrompe o processamento de um watch sem remover o registro.

```bash
sift watch stop <PATH>
```

Exemplo:

```bash
sift watch stop ~/Downloads
```

Você pode iniciá-lo novamente mais tarde.

---

## `sift watch remove`

Remove completamente um registro de watch.

```bash
sift watch remove <PATH>
```

Exemplo:

```bash
sift watch remove ~/Downloads
```

---

# Comandos do daemon do Watch

O Watch usa um daemon em segundo plano para os watches registrados em execução.

## Status do daemon

```bash
sift watch daemon status
```

Mostra se o daemon está em execução e, quando disponível, seu PID/informações de início.

## Parar o daemon

```bash
sift watch daemon stop
```

Solicita um desligamento seguro do daemon.

## Executar daemon em primeiro plano

```bash
sift watch daemon run
```

Este é um comando interno/avançado usado pelo Sift ao iniciar o daemon e normalmente não é algo que os usuários precisam invocar manualmente.

### Suporte de plataforma

O daemon em segundo plano do Watch está atualmente implementado para plataformas Unix-like.

Em plataformas não suportadas, o Sift relata um erro explícito em vez de fingir que o Watch em segundo plano está em execução.

#### Windows

Não existe build oficial pra Windows (o `install.sh` e a release do GitHub só cobrem Linux e macOS). Dito isso, tudo exceto o `sift watch` é escrito sobre APIs portáveis do `std::fs` e a crate cross-platform `trash`, e o workspace inteiro compila limpo pra `x86_64-pc-windows-gnu` — `scan`/`organize`/`clean`/`doctor`/`history`/`undo`/`init`/`folders`/`config`/`explain` devem funcionar se você compilar a partir do código-fonte, embora isso nunca tenha rodado num Windows de verdade e não esteja coberto por CI. O `sift watch` especificamente não vai funcionar: o spawn do daemon em `watch::platform` é intencionalmente Unix-only (veja acima), então um build nativo do daemon pra Windows precisa de trabalho real de detach de processo (`CREATE_NEW_PROCESS_GROUP`/`DETACHED_PROCESS`) que ainda não foi feito nem testado numa máquina Windows.

---

# Por que o Watch espera antes de mover um novo arquivo

Um evento do sistema de arquivos não significa necessariamente que um arquivo terminou de chegar.

Por exemplo, um navegador pode criar um download e continuar escrevendo nele por vários segundos.

Por isso, o Sift usa uma janela de estabilidade antes que um candidato se torne elegível para organização.

O padrão atual é aproximadamente:

```text
2,5 segundos inalterados
```

O Sift observa o tamanho e a hora de modificação. Se qualquer um deles mudar, o temporizador de estabilidade é reiniciado.

Nomes comuns de downloads temporários são ignorados enquanto permanecem transitórios:

```text
.crdownload
.part
.download
.tmp
```

Quando um aplicativo renomeia o arquivo para o nome final, o evento final do sistema de arquivos pode se tornar um novo candidato.

---

# Classificação de arquivos

A classificação interna é determinística e insensível a maiúsculas/minúsculas por extensão.

O Sift não inspeciona o conteúdo dos arquivos para decidir a categoria.

| Categoria | Destino | Extensões |
|---|---|---|
| Documents | `Documents/` | `pdf`, `txt`, `md`, `markdown`, `rtf`, `doc`, `docx`, `odt`, `xls`, `xlsx`, `ods`, `ppt`, `pptx`, `odp`, `epub`, `mobi` |
| Images | `Images/` | `jpg`, `jpeg`, `png`, `gif`, `webp`, `svg`, `bmp`, `tiff`, `tif`, `heic`, `heif`, `avif`, `ico` |
| Audio | `Audio/` | `mp3`, `wav`, `flac`, `aac`, `m4a`, `ogg`, `opus`, `wma` |
| Video | `Video/` | `mp4`, `mov`, `mkv`, `avi`, `webm`, `m4v`, `mpg`, `mpeg`, `wmv` |
| Archives | `Archives/` | `zip`, `rar`, `7z`, `tar`, `gz`, `bz2`, `xz`, `tgz`, `tbz2`, `txz` |
| 3D | `3D/` | `stl`, `obj`, `3mf`, `step`, `stp`, `blend`, `blend1`, `fbx`, `glb`, `gltf`, `dae`, `ply` |
| Code | `Code/` | `js`, `jsx`, `mjs`, `cjs`, `ts`, `tsx`, `py`, `rs`, `go`, `php`, `dart`, `sh`, `bash`, `zsh`, `fish`, `lua`, `java`, `c`, `h`, `cc`, `cpp`, `cxx`, `hpp`, `cs`, `rb`, `swift`, `kt`, `kts`, `scala`, `vue`, `svelte` |
| Data | `Data/` | `json`, `jsonl`, `yaml`, `yml`, `toml`, `csv`, `tsv`, `xml`, `sql`, `sqlite`, `sqlite3`, `db`, `parquet`, `ndjson` |
| Other | `Other/` | Qualquer outro arquivo comum não protegido |
| Junk | Candidato à lixeira | `tmp`, `swp`, `swo` |

Arquivos comuns desconhecidos são movidos intencionalmente para `Other/` em vez de ficarem como arquivos ambíguos não classificados.

---

# Proteção de projetos e travessia

O Sift reconhece marcadores comuns de projeto de software.

Um diretório contendo qualquer um destes é considerado raiz de projeto de software:

```text
.git
Cargo.toml
package.json
pyproject.toml
pubspec.yaml
```

A travessia recursiva também para em diretórios conhecidos de saída de build/dependência:

```text
node_modules
target
.venv
```

O Sift também evita descer para seus próprios diretórios de categoria:

```text
Documents
Images
Audio
Video
Archives
3D
Code
Data
Other
```

Isso impede aninhamento repetido, como:

```text
Documents/Documents/file.pdf
```

---

# Configuração com `.sift.toml`

O Sift pode substituir a classificação interna com regras explícitas.

Gere um arquivo inicial:

```bash
sift init ~/Downloads
```

Exemplo:

```toml
[[rules]]
name = "manter exportações de banco aqui"
pattern = "*.db"
action = "Move"
destination = "Database"
priority = 100
enabled = true
description = "Mantém arquivos de banco em uma pasta dedicada"

[[rules]]
name = "nunca mexer em notas"
pattern = "notes.md"
action = "Skip"
priority = 200
enabled = true

[[rules]]
name = "arquivos temporários do editor"
pattern = "*.swp"
action = "Trash"
priority = 300
enabled = true
```

## Parâmetros da regra

Cada bloco `[[rules]]` suporta:

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---:|---|
| `name` | string | Sim | Nome legível da regra |
| `pattern` | string | Sim | Padrão de correspondência do nome do arquivo |
| `action` | string | Sim | `Move`, `Trash` ou `Skip` |
| `destination` | string | Para `Move` | Diretório de destino relativo dentro do alvo |
| `priority` | integer | Sim | Valores maiores são executados primeiro |
| `enabled` | boolean | Não | Define se a regra participa. O padrão é `false` quando omitido |
| `description` | string | Não | Motivo legível exibido para a decisão |

### Precedência das regras

Regras habilitadas são avaliadas da maior `priority` para a menor.

A primeira regra que corresponder vence.

```text
regra habilitada explícita
        ↓
classificação interna
```

Por exemplo, embora JSON normalmente mapeie para `Data/`, esta regra deixa os arquivos JSON intocados:

```toml
[[rules]]
name = "manter json aqui"
pattern = "*.json"
action = "Skip"
priority = 1000
enabled = true
```

### Correspondência de padrões na v0.1

A correspondência de regras é baseada no nome do arquivo e intencionalmente simples na versão atual.

Padrões como estes são adequados:

```text
*.zip
*.json
notes.md
```

Atualmente não é um motor glob completo do sistema de arquivos. Evite depender de semântica avançada de glob.

### `Move`

```toml
[[rules]]
name = "arquivos sqlite"
pattern = "*.sqlite"
action = "Move"
destination = "Databases"
priority = 100
enabled = true
```

Os destinos devem permanecer abaixo do diretório alvo selecionado.

Destinos inseguros são rejeitados/ignorados. Em particular, destinos não podem usar:

```text
caminhos absolutos
..
.
componentes de raiz
escapes de link simbólico
```

### `Skip`

```toml
[[rules]]
name = "deixar anotações markdown em paz"
pattern = "*.md"
action = "Skip"
priority = 100
enabled = true
```

### `Trash`

```toml
[[rules]]
name = "temporários do editor"
pattern = "*.swp"
action = "Trash"
priority = 100
enabled = true
```

Em `organize`, as regras `Move`, `Trash` e `Skip` têm significado.

Em `clean`, `Trash` e `Skip` são relevantes; uma regra `Move` não é aplicada como movimento pelo planejador de limpeza.

---

# Estratégias de organização baseadas em metadados

Além de `strategy = "type"` (classificação por extensão), o `.sift.toml` suporta cinco estratégias que renderizam um destino a partir de uma string `template`:

| Estratégia | Fonte de metadados | Exemplo de template |
|---|---|---|
| `date` | data de modificação do arquivo no sistema de arquivos | `"{year}/{month}"` |
| `audio` | metadados de tag (ID3v2, Vorbis comments, átomos MP4, ...) | `"{artist}/{album}"` |
| `video` | informações do container MP4/MOV | `"{resolution}/{year}"` |
| `photos` | metadados EXIF | `"{camera}/{year}"` |
| `documents` | `/Info` do PDF ou `docProps/core.xml` do Office | `"{author}/{year}"` |

`[[rules]]` sempre vence qualquer uma dessas, exatamente como com `type`.

## `date`

```toml
[organize]
strategy = "date"
template = "{year}/{month}"
```

Placeholders suportados: `{year}`, `{month}`, `{day}`. `organize.date_source` tem como padrão (e atualmente só suporta) `"modified"`.

## `audio`

```toml
[organize]
strategy = "audio"
template = "{artist}/{album}"
```

Lê metadados de tag via [`lofty`](https://crates.io/crates/lofty) — mp3, flac, m4a, ogg, opus, wav, wma, aiff, entre outros. Placeholders suportados: `{artist}`, `{album}`, `{album_artist}`, `{genre}`, `{track}`, `{title}`, `{year}`.

## `video`

```toml
[organize]
strategy = "video"
template = "{resolution}/{year}"
```

Por padrão, lê metadados de nível de container via um parser em Rust puro (só MP4/MOV, sem dependência de nenhuma ferramenta externa). Se o `ffprobe` estiver instalado e no `PATH`, o Sift passa a usá-lo automaticamente — os valores de `{width}`/`{height}`/`{resolution}`/`{codec}`/`{year}` são idênticos nos dois casos (`{codec}` sempre renderiza o fourcc bruto, ex: `avc1`, nunca o nome de codec mais "amigável" do ffprobe, então um template escrito antes de instalar o ffmpeg nunca aponta pra outro lugar silenciosamente depois), mais suporte a mais containers e dois placeholders extras:

| Placeholder | Origem | Precisa de `ffprobe`? |
|---|---|---|
| `{width}`, `{height}`, `{resolution}` (`LARGURAxALTURA`) | dimensões da trilha de vídeo | Não |
| `{codec}` | fourcc bruto, ex: `avc1` | Não |
| `{year}` | data de criação do container, quando definida | Não |
| `{duration}` | duração em segundos inteiros | Sim |
| `{fps}` | taxa de quadros arredondada | Sim |

Nunca há risco de injeção de shell: o caminho do arquivo sempre vai como argumento separado do processo, nunca interpolado numa string de shell. Se o `ffprobe` não estiver instalado, `{duration}`/`{fps}` simplesmente ficam indisponíveis — veja abaixo.

## `photos`

```toml
[organize]
strategy = "photos"
template = "{camera}/{year}/{month}"
```

Lê metadados EXIF via [`kamadak-exif`](https://crates.io/crates/kamadak-exif) (Rust puro, sem ferramenta externa) — JPEG, TIFF, HEIF/HEIC, PNG e WebP são todos detectados automaticamente. Placeholders suportados: `{camera}` (Make+Model, deduplicado quando o modelo já repete o fabricante, ex: `"Canon EOS R5"` em vez de `"Canon Canon EOS R5"`), `{year}`, `{month}`, `{day}` (de `DateTimeOriginal` — a data de captura, nunca o mtime do arquivo).

Deliberadamente não existe placeholder `{gps}`/localização: embutir as coordenadas de captura no nome de uma pasta é um jeito fácil de vazar onde a foto foi tirada sem querer.

## `documents`

```toml
[organize]
strategy = "documents"
template = "{author}/{year}"
```

Lê metadados de arquivos PDF e Office (docx/xlsx/pptx), normalizados no mesmo formato independente do tipo de arquivo:

| Formato | Origem |
|---|---|
| PDF | o dicionário `/Info`, via a crate em Rust puro [`lopdf`](https://crates.io/crates/lopdf) |
| docx/xlsx/pptx | `docProps/core.xml` dentro do zip, via as crates em Rust puro [`zip`](https://crates.io/crates/zip) + [`roxmltree`](https://crates.io/crates/roxmltree) |

Placeholders suportados: `{author}`, `{title}`, `{year}`, `{month}`, `{day}` (a data de criação registrada no próprio documento — `CreationDate` do PDF, `dcterms:created` do Office — nunca o mtime do arquivo).

## Metadado ausente vira skip, nunca um chute

Se um arquivo não tem uma tag/campo que o template referencia (um mp3 sem tag, uma foto sem dado EXIF, um PDF sem dicionário `/Info`), esse arquivo é **ignorado (skip)** com um motivo claro — o Sift nunca inventa uma pasta de fallback como `Unknown Artist/`. Rode `sift explain <arquivo>` para ver exatamente qual campo estava indisponível.

## Passo a passo: uma estratégia diferente por pasta

O `.sift.toml` mora dentro do diretório que ele governa (`PATH/.sift.toml` — veja [Configuration lookup](#configuration-lookup) mais abaixo), não em um arquivo central único. Isso significa que cada pasta de nível superior que você organiza pode rodar sua própria estratégia, independente das outras — sua biblioteca de música por tag, seu acervo de fotos por câmera/data, seus PDFs por autor, tudo ao mesmo tempo:

```text
~/Music/.sift.toml       strategy = "audio"      → {artist}/{album}
~/Pictures/.sift.toml    strategy = "photos"     → {camera}/{year}/{month}
~/Documents/.sift.toml   strategy = "documents"  → {author}/{year}
~/Downloads/.sift.toml   strategy = "type"       → Documents/Images/Audio/... padrão (ou nenhum arquivo)
```

Configure uma do início ao fim. Gere um arquivo inicial:

```bash
sift init ~/Music
```

Substitua o exemplo de `[[rules]]` gerado em `~/Music/.sift.toml` por:

```toml
[organize]
strategy = "audio"
template = "{artist}/{album}"
```

Pré-visualize o plano e depois aplique:

```bash
sift organize ~/Music
sift organize ~/Music --apply
```

Antes:

```text
~/Music/
├── 01 Track One.mp3
├── 02 Track Two.mp3
└── live_bootleg.flac
```

Depois — os destinos vêm das tags de cada arquivo, nunca do nome do arquivo:

```text
~/Music/
├── Daft Punk/
│   └── Discovery/
│       ├── 01 Track One.mp3
│       └── 02 Track Two.mp3
└── Radiohead/
    └── I Might Be Wrong/
        └── live_bootleg.flac
```

Um arquivo sem a tag `{artist}`/`{album}` é ignorado (skip), nunca jogado numa pasta chutada — rode `sift explain ~/Music/live_bootleg.flac` antes para ver exatamente qual tag seria usada ou está faltando.

O mesmo padrão `sift init <pasta>` → editar `.sift.toml` → `sift organize <pasta> --apply` vale para `~/Pictures` (`strategy = "photos"`, ex: `template = "{camera}/{year}/{month}"`) e `~/Documents` (`strategy = "documents"`, ex: `template = "{author}/{year}"`). A política de cada diretório só afeta os arquivos dentro dele.

## `audio`/`video`/`photos`/`documents` ainda não suportam `--recursive`

Diferente dos placeholders de largura fixa `{year}`/`{month}`/`{day}` do `date`, um valor de `{artist}`, `{camera}` ou `{author}` é texto livre — estruturalmente indistinguível de qualquer outro nome de pasta. Isso significa que hoje não há como detectar com segurança "esse diretório foi gerado por esta mesma política" e evitar reentrar nele numa segunda execução. Tanto `sift organize --recursive` quanto `sift watch add --recursive` recusam `strategy = "audio"`/`"video"`/`"photos"`/`"documents"` com um erro claro em vez de arriscar; um `sift organize` simples (não recursivo) e um `sift watch` não recursivo funcionam normalmente.

---

# Configuration lookup

For a command targeting `PATH`, Sift currently looks for configuration in this order:

```text
1. PATH/.sift.toml
2. platform config directory / sift / config.toml
3. built-in defaults
```

On a typical Linux system, the global path corresponds to something like:

```text
~/.config/sift/config.toml
```

Sift does **not** currently walk up parent directories looking for additional `.sift.toml` files.

For a recursive organize operation, the configuration selected for the command root is used for that operation.

Watch resolves configuration from the registered watch root while processing candidates.

---

# Safety model

Safety is part of Sift's architecture, not an optional mode.

## Dry-run first

```bash
sift organize ~/Downloads
```

shows the plan.

```bash
sift organize ~/Downloads --apply
```

executes it.

The same pattern applies to cleanup.

## Symlinks are not followed for organization decisions

Sift uses no-follow metadata for safety-sensitive filesystem checks.

A symlink, including a broken symlink, is treated as an occupied/protected filesystem entry rather than an empty destination.

## No silent overwrite

If the destination already exists, the move is skipped.

That includes an existing:

- regular file;
- directory;
- symlink;
- broken symlink.

## No automatic directory merge

Sift does not merge directory trees simply because destination names happen to match.

## Project roots are protected

Running organization against a recognized project root causes the target contents to be protected instead of dismantled into `Code/`, `Data/`, and other categories.

## Live revalidation before mutation

Planning-time assumptions are not blindly trusted during execution.

Sift re-checks filesystem state before mutating so that a destination created between preview and execution can block the move safely.

## No copy-delete fallback

If a rename cannot be performed safely, Sift does not silently turn it into a copy-then-delete operation.

## Trash instead of permanent deletion

Cleanup uses the operating system trash instead of issuing a permanent `rm` on your behalf.

---

# JSON output and scripting

The following command groups currently expose `--json`:

```text
scan
organize
clean
doctor
watch list
watch status
```

Examples:

```bash
sift scan ~/Downloads --json
sift organize ~/Downloads --json
sift doctor ~/Downloads --recursive --json
sift watch list --json
```

This makes Sift useful as both an interactive CLI and a building block for scripts and other tools.

Example with `jq`:

```bash
sift scan ~/Downloads --json | jq .
```

---

# Suggested workflows

## Keep Downloads manageable manually

```bash
sift organize ~/Downloads
sift organize ~/Downloads --apply
sift history
```

## Inspect before organizing

```bash
sift scan ~/Desktop --recursive
sift doctor ~/Desktop --recursive
sift organize ~/Desktop --recursive
```

## Create a custom Downloads policy

```bash
sift init ~/Downloads
$EDITOR ~/Downloads/.sift.toml
sift organize ~/Downloads
```

## Organize a library by its own metadata (music, photos, documents...)

Each folder's `.sift.toml` is independent, so `~/Music`, `~/Pictures`, and `~/Documents` can each run a different metadata-based `strategy` (tags, EXIF, document properties) at the same time — see [Passo a passo: uma estratégia diferente por pasta](#passo-a-passo-uma-estratégia-diferente-por-pasta) for the full example.

## Turn an Inbox into a Smart Inbox

```bash
mkdir -p ~/Inbox
sift watch add ~/Inbox --auto-apply
sift watch start ~/Inbox
sift watch status ~/Inbox
```

From then on, new stable eligible files can be organized automatically.

## Organize existing files before enabling Watch

```bash
sift organize ~/Inbox
sift organize ~/Inbox --apply
sift watch add ~/Inbox --auto-apply
sift watch start ~/Inbox
```

This keeps the distinction clear:

```text
manual organize → existing content
watch           → future filesystem events
```

---

# Architecture

The core flow is intentionally separated into stages:

```text
Filesystem
   │
   ▼
Scanner
   │
   ▼
Classifier
   │
   ▼
Rules / policy
   │
   ▼
Planner
   │
   ▼
Review
   │
   ▼
Executor
   │
   ▼
History
   │
   ▼
Undo
```

The scanner does not mutate the filesystem.

The planner decides what should happen.

The executor is responsible for applying an explicitly authorized plan and revalidating safety assumptions.

Watch reuses the same organization authority instead of inventing a separate, weaker organizer.

---

# Project layout

```text
src/
├── classifier.rs   deterministic extension → category classification
├── cli.rs          command-line interface and parameters
├── config.rs       .sift.toml rules and configuration lookup
├── domain.rs       shared domain types
├── executor.rs     filesystem mutation + execution safeguards
├── fs.rs           filesystem helpers
├── history.rs      history persistence and undo
├── planner.rs      organization and cleanup planning
├── render.rs       human-readable output
├── scanner.rs      scanning, diagnosis, recursive traversal boundaries
├── utils.rs        shared utility helpers
└── watch/
    ├── daemon.rs
    ├── eligibility.rs
    ├── engine.rs
    ├── platform.rs
    ├── registry.rs
    └── stability.rs
```

---

# Development

Run formatting:

```bash
cargo fmt --check
```

Run Clippy:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Run tests:

```bash
cargo test
```

Build release binary:

```bash
cargo build --release
```

---

# Contributing

Contributions are welcome.

Before opening a pull request, please make sure the project is formatted, lint-clean, and passing tests.

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for project contribution guidance and [`SECURITY.md`](SECURITY.md) for security reports.

---

# License

Sift is licensed under the [Apache License 2.0](LICENSE).

Copyright 2026 Sérgio Cardoso.