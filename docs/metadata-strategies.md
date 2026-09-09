# Estratégias de organização baseadas em metadados

Documento técnico detalhado de tudo que foi implementado nesta sessão: quatro
novas estratégias de organização (`audio`, `video`, `photos`, `documents`),
integração opcional com `ffprobe`, e infraestrutura de instalação (workflow
de release + `install.sh`). Escrito para servir de referência de arquitetura
e de histórico das decisões tomadas — não é o README (esse é o guia de uso
para quem instala o sift; este documento é sobre *como e por que* foi
construído).

## Índice

1. [Contexto e ponto de partida](#contexto-e-ponto-de-partida)
2. [Arquitetura comum às quatro estratégias](#arquitetura-comum-às-quatro-estratégias)
3. [`strategy = "audio"`](#strategy--audio)
4. [`strategy = "video"` e a integração opcional com `ffprobe`](#strategy--video-e-a-integração-opcional-com-ffprobe)
5. [`strategy = "photos"`](#strategy--photos)
6. [`strategy = "documents"`](#strategy--documents)
7. [Por que nenhuma delas suporta `--recursive`](#por-que-nenhuma-delas-suporta---recursive)
8. [Instalação: workflow de release + `install.sh`](#instalação-workflow-de-release--installsh)
9. [Testes e fixtures](#testes-e-fixtures)
10. [Mapa de arquivos tocados](#mapa-de-arquivos-tocados)
11. [Decisões explícitas do usuário (registro)](#decisões-explícitas-do-usuário-registro)
12. [Verificação final](#verificação-final)

---

## Contexto e ponto de partida

Antes desta sessão, o sift já tinha duas estratégias de organização em
`.sift.toml`:

- `strategy = "type"` — classificação por extensão (o padrão original).
- `strategy = "date"` — organiza pelo `mtime` do arquivo, renderizado por um
  `Template` com placeholders `{year}`/`{month}`/`{day}` de largura fixa
  (dígitos).

O código já deixava uma pista explícita de que mais estratégias
"metadata-driven" viriam: um comentário em `src/config.rs` dizia que um
futuro `media`/`photos`/`audio` reusaria o mecanismo de `Template`, "never a
bespoke path-building routine". Esta sessão implementou exatamente isso — e
foi além, com `video` e `documents` também.

Importante: até esta sessão, o sift **nunca lia conteúdo de arquivo**. O
classificador (`classifier.rs`) só olha a extensão; a estratégia `date` só
lê o `mtime` via `symlink_metadata`. As quatro estratégias novas são as
primeiras a abrir e parsear o *conteúdo* dos arquivos (tags de áudio,
containers de vídeo, EXIF, PDF/Office).

## Arquitetura comum às quatro estratégias

### `MetadataTemplate`: o irmão do `Template` de `date` para texto livre

O `Template` original (usado só por `date`) tem uma propriedade importante:
todo placeholder renderiza para um valor de **largura fixa e só-dígitos**
(`{year}` → 4 dígitos, `{month}`/`{day}` → 2 dígitos). Isso é o que permite
uma checagem estrutural barata (`component_could_be_generated`) que evita o
organize recursivo reentrar numa pasta que ele mesmo gerou (ex: nunca
descer de novo em `2026/09/` e aninhar `2026/09/2026/09/`).

Tags de áudio, nomes de câmera, autores de documento etc. são **texto
livre, de largura variável** — essa checagem estrutural não existe pra
eles. Por isso foi criado um tipo irmão, `MetadataTemplate`
(`src/config.rs`), com a mesma forma de fora (literal + `{placeholder}`,
mesma validação de path seguro) mas:

- Cada estratégia (`audio`/`video`/`photos`/`documents`) tem seu próprio
  conjunto de placeholders válidos (`AUDIO_FIELDS`, `VIDEO_FIELDS`,
  `PHOTOS_FIELDS`, `DOCUMENT_FIELDS` — arrays de `MetadataField`).
- **Regra de parse**: dois placeholders de largura variável não podem ficar
  colados sem um literal separador entre eles (`"{artist}{album}"` é
  rejeitado) — isso mantém o casamento de padrão determinístico sem precisar
  de backtracking.
- **Sanitização** (`sanitize_metadata_value`): todo valor de placeholder
  passa por uma limpeza antes de virar componente de path — troca `/`, `\`
  e bytes de controle por `_`, apara `.` nas pontas (pra nunca virar `.`/`..`
  sozinho), corta em 150 bytes. Um valor que sanitiza pra string vazia (ex:
  uma tag literalmente `".."`) conta como "indisponível", nunca vira um
  componente de path vazio ou perigoso.
- O caminho renderizado final passa pela mesma validação de segurança
  (`validate_rule_destination`) que as regras `[[rules]]` usam — defesa em
  profundidade, igual ao `Template` original já fazia.

`EffectivePolicy.metadata_template: Option<MetadataTemplate>` é um único
campo compartilhado pelas quatro estratégias (nunca populado ao mesmo tempo
que `template`/`date_source`, que continuam exclusivos de `date`).

### "Metadado ausente → skip, nunca um chute"

Essa é a regra mais repetida do projeto inteiro nesta sessão, em todas as
quatro estratégias: se um arquivo não tem o campo que o template referencia
(um mp3 sem tag de artista, um PDF sem `/Info`, uma foto sem EXIF), o
arquivo é **pulado** com um motivo explicativo (`"audio metadata
unavailable: missing artist metadata"` etc.) — o sift nunca cria uma pasta
de fallback tipo `Unknown Artist/`. Isso espelha exatamente o que `date`
já fazia (`DateUnavailable`), então é consistência, não uma regra nova.

Cada estratégia tem seu par de `DecisionCause` (`planner.rs`):
`AudioMatched`/`AudioUnavailable`, `VideoMatched`/`VideoUnavailable`,
`PhotoMatched`/`PhotoUnavailable`, `DocumentMatched`/`DocumentUnavailable`.

### O padrão mecânico repetido 4 vezes

Cada estratégia nova seguiu **exatamente** a mesma receita, o que deixou a
implementação de `photos` e `documents` bem mais rápida que `audio`/`video`
(que estabeleceram o padrão):

1. `src/config.rs`: struct `XMetadata`, variantes novas em `MetadataField`,
   array `X_FIELDS`, `MetadataTemplate::parse_x`, `MetadataTemplate::render_x`
   (e um braço `unreachable!()` novo em *todas* as outras `render_*`, já que
   o Rust força isso por exaustividade — é o compilador guiando pra cada
   ponto que precisa mudar).
2. `src/metadata.rs`: `extract_x_metadata(path) -> Result<XMetadata, String>`
   — a única função que efetivamente lê o conteúdo do arquivo.
3. `src/planner.rs`: `classify_for_x`, `resolve_x_action` (reaproveitando o
   helper `resolve_metadata_action` compartilhado — colisão, multi-nível de
   `CreateDir`), `plan_entry_x`, `plan_organize_x`, e um braço novo nos 3
   pontos de dispatch (`plan_entry_with_strategy`, `plan_with_strategy`,
   `plan_with_strategy_recursive`).
4. `src/explain.rs` e `src/render.rs`: campo `x_metadata` na `Explanation`,
   braço no resumo de causa, exibição no `sift explain`.
5. Nenhuma mudança em `src/watch/`: a restrição de `--recursive` e o
   registro/hot-reload de watch já são genéricos via
   `OrganizeStrategy::supports_recursive()` — só listar `Type`/`Date` como
   `true` já basta, tudo o resto (fail-closed no daemon, recusa no `watch
   add --recursive`) funciona sem tocar em `watch/`.

---

## `strategy = "audio"`

```toml
[organize]
strategy = "audio"
template = "{artist}/{album}"
```

- **Extração**: `src/metadata.rs::extract_audio_metadata`, via a crate
  [`lofty`](https://crates.io/crates/lofty) (Rust puro). Cobre mp3, flac,
  m4a, ogg, opus, wav, wma, aiff e mais — `lofty::read_from_path` detecta o
  formato sozinho. Usa `tagged_file.primary_tag().or_else(|| first_tag())`
  e o trait `Accessor` do lofty pra puxar `artist`/`album`/`genre`/
  `track`/`title`/`date().year`; `album_artist` vem via
  `ItemKey::AlbumArtist` (não faz parte do `Accessor` genérico).
- **Placeholders**: `{artist}`, `{album}`, `{album_artist}`, `{genre}`,
  `{track}`, `{title}`, `{year}`.
- Um arquivo sem tag nenhuma retorna `AudioMetadata` com todos os campos
  `None` — não é erro, é "sem metadado", igual a qualquer outro tipo.

---

## `strategy = "video"` e a integração opcional com `ffprobe`

```toml
[organize]
strategy = "video"
template = "{resolution}/{year}"
```

Esta é a estratégia mais elaborada — tem dois backends.

### Backend 1 (sempre disponível): parser Rust puro

`extract_video_metadata_via_mp4_crate` usa a crate
[`mp4`](https://crates.io/crates/mp4) pra ler `width`/`height`/codec fourcc
da primeira trilha de vídeo e o `creation_time` do box `mvhd` (epoch do
QuickTime, 1904-01-01 — convertido pra epoch Unix subtraindo
`2_082_844_800`; `creation_time == 0`, comum em muitos encoders, vira "sem
ano" via `checked_sub` retornando `None` no underflow). Só entende
MP4/MOV. Nunca seta `duration_seconds`/`fps`.

### Backend 2 (opcional): `ffprobe`

Se o binário `ffprobe` existir, o sift tenta ele **primeiro**:
`extract_video_metadata_via_ffprobe` roda `ffprobe -v quiet -print_format
json -show_format -show_streams <arquivo>` via `std::process::Command` (o
caminho do arquivo vai como argumento separado do processo — nunca
interpolado em string, sem risco de injeção de shell) e parseia o JSON com
`serde_json::Value` (já era dependência do projeto, zero dependência nova
só pra isso).

Se o `ffprobe` não existir no PATH, ou existir mas falhar por qualquer
motivo (exit code, JSON inválido, sem stream de vídeo), o sift cai
silenciosamente pro backend 1 — **nunca propaga o erro do ffprobe direto**,
sempre tenta o fallback antes de desistir de vez.

**Decisão de design deliberada**: `{codec}` sempre renderiza o fourcc bruto
(ex: `"avc1"`) — o ffprobe também expõe isso via `codec_tag_string`, então
o sift usa esse campo e **não** `codec_name` (que daria `"h264"`, mais
"amigável" mas semanticamente diferente). Isso garante que um template já
configurado nunca aponta pra uma pasta diferente só porque a pessoa
instalou o ffmpeg depois — os campos compartilhados (`width`/`height`/
`resolution`/`codec`/`year`) são **idênticos** nos dois backends para o
mesmo arquivo (validado pelo teste
`fallback_backend_agrees_on_shared_fields_but_never_sets_duration_or_fps`).

Dois placeholders **só existem via `ffprobe`**:
- `{duration}` — segundos inteiros, de `format.duration`.
- `{fps}` — de `r_frame_rate` (fração tipo `"25/1"` ou `"30000/1001"`),
  arredondado.

Se o template usa `{duration}`/`{fps}` e o `ffprobe` não está instalado,
esses campos ficam `None` no backend 1 → o arquivo é pulado com motivo
claro (nunca um valor inventado) — a mesma regra "ausente = skip" aplicada
de forma consistente entre os dois backends.

### Testabilidade sem mexer no `PATH` real

Pra testar o caminho "sem ffprobe" de forma determinística, sem risco de
condição de corrida com outros testes do mesmo binário rodando em paralelo
(que legitimamente precisam achar o `ffprobe` de verdade), foi criada uma
seam de teste pública:
`extract_video_metadata_with_ffprobe_search_path(path, search_path)` —
resolve o binário `ffprobe` manualmente varrendo os diretórios de
`search_path` (em vez de deixar o `Command::new("ffprobe")` procurar no
`PATH` real do processo). `extract_video_metadata` pública é só um atalho
que chama essa variante com `std::env::var("PATH")`.

### CI

`.github/workflows/ci.yml` ganhou um passo `apt-get install -y ffmpeg`
antes do `cargo test`, pra que o caminho `ffprobe` seja exercitado de
verdade em CI a cada execução, não só localmente.

---

## `strategy = "photos"`

```toml
[organize]
strategy = "photos"
template = "{camera}/{year}/{month}"
```

- **Extração**: `src/metadata.rs::extract_photo_metadata`, via a crate
  [`kamadak-exif`](https://crates.io/crates/kamadak-exif) (nome do pacote;
  o nome da lib no código é `exif`). `exif::Reader::new().read_from_container(...)`
  auto-detecta JPEG, TIFF, HEIF/HEIC, PNG e WebP a partir dos bytes — sem
  precisar de ferramenta externa.
- **Placeholders**: `{camera}` (combina `Make`+`Model` do EXIF, com
  deduplicação: se `Model` já contém o `Make` — comum em várias marcas,
  ex. Canon grava `Model = "Canon EOS R5"` — usa só `Model`, evitando
  `"Canon Canon EOS R5"`), `{year}`, `{month}`, `{day}` (de
  `DateTimeOriginal`, a data de captura — nunca o mtime do arquivo).
- **Sem `{gps}`/localização** — decisão explícita do usuário (ver seção de
  decisões abaixo): embutir coordenadas de captura no nome de uma pasta é
  um jeito fácil de vazar onde a foto foi tirada sem querer.
- Um arquivo sem EXIF (comum em PNG/WebP, ou um JPEG com metadado
  removido) retorna `PhotoMetadata` toda `None` — não é erro
  (`exif::Error::NotFound` é tratado como esse caso).

---

## `strategy = "documents"`

```toml
[organize]
strategy = "documents"
template = "{author}/{year}"
```

A única das quatro que lida com **dois formatos completamente diferentes**,
despachados por extensão (`extract_document_metadata`):

### PDF — via `lopdf`

`extract_pdf_metadata` abre o documento com
[`lopdf`](https://crates.io/crates/lopdf) (`default-features = false` —
sem puxar `chrono`/`rayon`, que o crate ativa por padrão mas o sift não
precisa) e lê o dicionário `/Info` do trailer (`Author`, `Title`,
`CreationDate`). Duas funções auxiliares:

- `pdf_info_string`: decodifica o valor — PDF tem duas codificações de
  string possíveis: UTF-16BE com BOM `FE FF` no início (usado pra texto
  fora de Latin-1), ou bytes crus (decodificados como UTF-8 lossy, preciso
  o bastante pro caso comum ASCII).
- `parse_pdf_date`: o formato de data do PDF é `"D:YYYYMMDDHHmmSS..."` —
  extrai os 8 primeiros dígitos após o `"D:"`.

Um PDF sem `/Info` (válido, ainda que incomum) retorna `DocumentMetadata`
toda `None`, não erro.

### Office (docx/xlsx/pptx) — via `zip` + `roxmltree`

Todo arquivo Office moderno é, por baixo, um zip com XML dentro. Já que a
única extração é do `docProps/core.xml` (a mesma parte em qualquer docx/
xlsx/pptx), `extract_office_metadata` abre o zip com
[`zip`](https://crates.io/crates/zip) (`default-features = false`, só a
feature `deflate-flate2-zlib-rs` — implementação de zlib em Rust puro, sem
puxar bzip2/lzma/zstd/aes que o Office nunca usa) e parseia o XML com
[`roxmltree`](https://crates.io/crates/roxmltree), procurando por
`dc:creator`/`dc:title`/`dcterms:created` (namespaces Dublin Core / DCMI
Terms) via `core_properties_text` (casa por par namespace+nome local, não
por string crua, pra não colidir por acidente com outro elemento do mesmo
nome em outro namespace).

- `parse_w3cdtf_date`: o `dcterms:created` vem em ISO 8601/W3CDTF (ex:
  `"2023-11-15T09:00:00Z"`) — extrai a parte antes do `T` e quebra em
  `year-month-day`.
- Um arquivo sem `docProps/core.xml` no zip (`zip::result::ZipError::FileNotFound`)
  também retorna `DocumentMetadata` toda `None`, não erro; um zip
  genuinamente corrompido/ilegível *é* um erro real.

### Placeholders

`{author}`, `{title}`, `{year}`, `{month}`, `{day}` — os mesmos nomes,
independente do arquivo ser PDF ou Office (`DocumentMetadata` é uma única
struct normalizada; quem chama nunca precisa saber qual dos dois backends
respondeu).

### Um detalhe de clippy

O `rustc`/`clippy` desta toolchain (1.98) sinalizou `chunks_exact(2)` como
preterido em favor de `[T]::as_chunks::<2>()` (estabilizada recentemente)
na decodificação UTF-16BE do PDF — trocado sem mudança de comportamento.

---

## Por que nenhuma delas suporta `--recursive`

Isso vale pras quatro (`audio`/`video`/`photos`/`documents`), é a mesma
razão, e foi uma decisão discutida explicitamente com o usuário antes de
implementar `audio`/`video` (ver seção de decisões).

O organize recursivo de `date` evita reentrar na própria pasta gerada
(`2026/09/`) porque consegue checar estruturalmente: todo componente de
`{year}/{month}` é dígitos de largura fixa, então dá pra perguntar "essa
pasta poderia ter sido gerada por este template, pra alguma data válida?"
sem ambiguidade.

Um valor de `{artist}`, `{camera}` ou `{author}` é texto livre — não tem
como distinguir estruturalmente uma pasta `"Pink Floyd"` gerada pelo sift
de uma pasta `"Pink Floyd"` que já existia por outro motivo qualquer. Sem
essa distinção, não tem como implementar a mesma proteção contra
reentrância sem risco real de aninhamento infinito (`Artist/Album/Artist/
Album/...`) numa segunda execução.

Duas alternativas foram cogitadas e descartadas nesta sessão:
- **Marcador de pasta gerada** (arquivo oculto tipo `.sift-generated`
  escrito pelo executor em toda pasta criada) — resolveria com precisão,
  mas exigiria tocar no executor (hoje só mexe no arquivo sendo
  organizado) e adicionar um efeito colateral novo ao `Op::CreateDir`.
  Descartado por enquanto — anotado como possível v2.
- Heurística de casamento estrutural pra texto livre — analisada e
  rejeitada: qualquer nome de pasta não-vazio "poderia" ter sido gerado
  por um placeholder de texto livre, então a heurística degenerava pra
  "não desce em nada", inútil na prática.

A solução adotada foi mais simples e honesta: **recusar `--recursive`**
com um erro claro, em vez de arriscar. Implementado de forma centralizada
via `OrganizeStrategy::supports_recursive()` (só `Type` e `Date` retornam
`true`), checado em três lugares:
1. `cmd_organize` (planner.rs) — recusa antes mesmo de montar o plano.
2. `plan_with_strategy_recursive` — defesa em profundidade: mesmo que
   outro chamador pule o passo 1, o resultado é um plano que só pula tudo
   com motivo claro, nunca organiza nada.
3. `cmd_watch_add`/`daemon.rs::refresh_policy` — recusa registrar um watch
   `--recursive` com uma dessas quatro estratégias, e também detecta (fail
   closed, suspende o watch) se um `.sift.toml` for editado *depois* pra
   trocar de `type`/`date` recursivo pra uma dessas quatro enquanto o
   watch já está rodando.

Organize não-recursivo e watch não-recursivo funcionam normalmente com as
quatro — a restrição é só sobre descer em subdiretórios.

---

## Instalação: workflow de release + `install.sh`

Motivado por uma pergunta separada do usuário: "dá pra instalar o sift via
`curl | sh`?" — e, depois, "o sift deveria usar o ffprobe quando
disponível?" (sim, ver seção de vídeo acima).

### `.github/workflows/release.yml`

Novo workflow, disparado por push de tag `v*.*.*`. Usa duas actions de
terceiros bem estabelecidas (confirmadas existentes e ativas via a API
pública do GitHub antes de referenciá-las):
[`taiki-e/create-gh-release-action@v1`](https://github.com/taiki-e/create-gh-release-action)
cria a release do GitHub a partir da tag, e
[`taiki-e/upload-rust-binary-action@v1`](https://github.com/taiki-e/upload-rust-binary-action)
compila e sobe o binário `sift` pra 4 alvos:

- `x86_64-unknown-linux-gnu` (runner `ubuntu-latest`, nativo)
- `aarch64-unknown-linux-gnu` (cross-compile via `cross`, que a action
  resolve sozinha)
- `x86_64-apple-darwin` / `aarch64-apple-darwin` (runner `macos-latest`,
  nativo)

Sem Windows — o daemon do watch já é Unix-only por design
(`src/watch/platform.rs`). Cada alvo gera `sift-<target>.tar.gz` +
`sift-<target>.sha256` (formato padrão `sha256sum`, conteúdo `<hash>
sift-<target>.tar.gz`) publicados na release.

**Este workflow nunca foi disparado de verdade nesta sessão** — nenhuma
tag `v*` foi criada/pushada. Cortar a primeira release é uma ação
separada e explícita, do usuário.

### `install.sh`

Script POSIX `sh` (sem bashismos) na raiz do repo:

```bash
curl -fsSL https://raw.githubusercontent.com/sergiocardoso/sift/main/install.sh | sh
```

1. Detecta SO (`uname -s`) e arquitetura (`uname -m`), monta o target.
2. Resolve a última release via a API pública do GitHub
   (`SIFT_API_URL`, sobrescrevível — usado pros testes locais).
3. Baixa o `.tar.gz` **e** o `.sha256` da mesma release
   (`SIFT_RELEASE_BASE_URL`, também sobrescrevível) e verifica o checksum
   antes de instalar qualquer coisa (`sha256sum -c` no Linux, `shasum -a
   256 -c` no macOS) — aborta se não bater.
4. Instala em `~/.local/bin/sift` — **nunca `sudo`, nunca
   `/usr/local/bin`** sem pedir. Avisa se o diretório não está no `PATH`.
5. Checa `ffprobe`: se ausente, detecta o gerenciador de pacotes
   disponível (`apt-get`/`dnf`/`pacman`/`brew`, nessa ordem) e **só roda o
   comando de instalação se**: o script está num terminal interativo real
   (`[ -t 0 ] && [ -t 1 ]`) **e** a pessoa confirma explicitamente (`y`).
   Em modo não-interativo (`curl | sh` sem TTY), ou resposta não
   afirmativa, só imprime a instrução e segue — nunca roda `sudo` sozinho
   sem confirmação em terminal real.

### Como foi testado sem uma release publicada

Como não existe release real ainda, o script foi validado de ponta a
ponta contra uma "release fake" local: o binário `release` já compilado
foi empacotado num `.tar.gz` + `.sha256` reais, servidos por um
`python3 -m http.server` local, com `SIFT_API_URL`/`SIFT_RELEASE_BASE_URL`
apontando pra esse servidor. Cenários cobertos:

- Caminho feliz completo (download → verify → install → binário funciona).
- Checksum malformado e checksum bem-formado mas errado → aborta, nada
  instalado, código de saída 1.
- `ffprobe` ausente do `PATH` (via um `PATH` fake montado só com os
  binários necessários, sem `ffprobe`) + modo não-interativo → só avisa,
  não tenta instalar.
- `ffprobe` ausente + `apt-get` detectável (fake) + não-interativo → mostra
  o comando exato, não roda.
- Modo interativo real (via `script`, alocando um pty) respondendo `y` →
  roda o comando de instalação (testado com `apt-get`/`sudo` fakes que só
  imprimem o que fariam, sem tocar no sistema real).
- Modo interativo respondendo `n` → pula, sem rodar nada.

---

## Testes e fixtures

Cada estratégia ganhou seu próprio arquivo de teste de integração em
`tests/`, seguindo a estrutura de `tests/date_strategy_integration.rs`
(parsing/validação de template, extração contra fixture real, planning —
destino/colisão/skip por metadado ausente, precedência de regras,
validação de config, `explain`, `config check`, recusa de `--recursive`):

| Arquivo | Testes |
|---|---|
| `tests/audio_strategy_integration.rs` | 27 |
| `tests/video_strategy_integration.rs` | 24 |
| `tests/photos_strategy_integration.rs` | 22 |
| `tests/documents_strategy_integration.rs` | 26 |

Total do projeto ao fim da sessão: **387 testes**, todos passando
(`cargo test --all-targets --all-features`), mais `cargo fmt --check` e
`cargo clippy --all-targets --all-features -- -D warnings` limpos — o
mesmo gate que `.github/workflows/ci.yml` roda.

### Fixtures reais em `tests/fixtures/`

Nenhum teste usa dados sintéticos/mockados pra extração de metadado — todo
teste roda contra um arquivo real, gerado uma vez com ferramentas de
sistema (`ffmpeg`, `exiftool`, `convert`/ImageMagick, e um script Python
pontual pra montar um `.docx` mínimo à mão) e commitado como fixture
binário:

- `tagged.mp3` / `untagged.mp3` — áudio com e sem tags ID3.
- `tiny.mp4` / `tiny_with_creation_time.mp4` — vídeo MP4 sem e com
  `creation_time` setado (esse segundo também tem `duration`/`fps`
  conhecidos, usado nos testes de `ffprobe`).
- `tiny.jpg` / `untagged.jpg` — foto com e sem EXIF (`Make`/`Model`/
  `DateTimeOriginal`).
- `tiny.pdf` / `untagged.pdf` — PDF com e sem `/Info`.
- `tiny.docx` — Office mínimo válido, montado à mão (zip +
  `[Content_Types].xml` + `_rels/.rels` + `word/document.xml` +
  `docProps/core.xml` + `docProps/app.xml`), já que não havia LibreOffice
  disponível pra gerar um de verdade.

---

## Mapa de arquivos tocados

```text
Cargo.toml / Cargo.lock      novas deps: lofty, mp4, kamadak-exif, lopdf, zip, roxmltree
src/config.rs                OrganizeStrategy, MetadataTemplate, X_FIELDS, XMetadata structs
src/metadata.rs              NOVO — todos os extract_x_metadata
src/planner.rs               classify_for_x, resolve_x_action, plan_entry_x, plan_organize_x
src/explain.rs               x_metadata na Explanation, dispatch
src/render.rs                exibição humana (sift explain / config check)
src/watch/mod.rs             cmd_watch_add recusa --recursive incompatível
src/watch/daemon.rs          refresh_policy recusa em hot-reload (fail-closed)
src/lib.rs                   pub mod metadata;
.github/workflows/ci.yml     passo de instalar ffmpeg
.github/workflows/release.yml NOVO — build+publish multi-plataforma
install.sh                   NOVO
tests/*_strategy_integration.rs  4 arquivos novos
tests/fixtures/              fixtures binários reais
README.md / README-PT.md     seção "Metadata-driven organize strategies" + instalação
docs/metadata-strategies.md  este documento
```

---

## Decisões explícitas do usuário (registro)

Pra manter rastreável *por que* as coisas ficaram do jeito que ficaram:

1. **Vídeo, v1**: parser Rust puro (MP4/MOV) em vez de `ffprobe`
   obrigatório — decisão inicial, antes de existir a opção de instalador.
2. **`{gps}` em `photos`**: excluído deliberadamente — risco de vazar
   localização de captura no nome de uma pasta sem a pessoa perceber.
3. **Campos de `photos`, v1**: `{camera}`/`{year}`/`{month}`/`{day}` — sem
   ISO/abertura/velocidade do obturador por enquanto (mencionados como
   possível extensão futura).
4. **Instalador**: binário pré-compilado (não `cargo install --git` na
   hora) — instalação quase instantânea, ao custo de precisar manter o
   workflow de release.
5. **`ffprobe` no instalador**: detectar e *oferecer* instalar com
   confirmação explícita — nunca instalar silenciosamente, nunca em modo
   não-interativo.
6. **Uso de `ffprobe` no sift**: sim, com fallback automático pro parser
   Rust puro — reabre parcialmente (só pra `video`) a decisão inicial de
   evitar subprocesso, mas de forma opt-in e sem regressão pra quem não
   tem ffmpeg instalado.
7. **Próximo metadado depois de `photos`**: `documents` (PDF/Office) — as
   alternativas cogitadas (mais campos em `audio`, IPTC/XMP em `photos`)
   ficaram de fora desta rodada, registradas como possíveis próximos
   passos.

---

## Verificação final

Comandos que qualquer um pode rodar pra confirmar o estado do projeto ao
fim desta sessão:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features   # 387 passed
sh -n install.sh                          # sintaxe válida
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))"
```

Smoke tests manuais rodados contra a CLI real (binário `release`) durante
a sessão, um por estratégia, incluindo `explain`/`config check`/
`organize --apply` com arquivos de verdade — não só os testes automatizados.

Nada foi commitado durante esta sessão; todo o trabalho está no working
tree, pronto para revisão antes de qualquer commit.
