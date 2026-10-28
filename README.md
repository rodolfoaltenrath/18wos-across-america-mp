# Across America: ferramentas e protótipo multiplayer em Rust

Repositório: [rodolfoaltenrath/18wos-across-america-mp](https://github.com/rodolfoaltenrath/18wos-across-america-mp).
Fontes próprios sob [Apache-2.0](LICENSE); arquivos do jogo não acompanham o projeto.

Workspace desenvolvido dentro da instalação original Steam de **18 Wheels of
Steel: Across America**. Todo o código do projeto, incluindo ferramentas de
automação e o programa de teste da DLL, está em Rust. Os blocos PowerShell abaixo
são comandos de terminal para compilar e executar os programas.

O projeto contém ferramentas de assets, transporte de coordenadas e uma bancada
de descoberta de memória em Rust. Uma cadeia de posição foi validada na instalação
local com movimento, reinício do jogo e comparação exata com o salvamento.
A DLL x86 já foi carregada no jogo sob Proton e transmitiu coordenadas reais
ao servidor nativo do Fedora, com recepção por outro cliente de teste.
A criação/movimentação de caminhões remotos continua pendente.

O plano completo, os critérios de conclusão e o procedimento para continuar em
outro computador estão em [ROADMAP.MD](ROADMAP.MD). As orientações de autonomia
e continuidade do desenvolvimento estão em [AGENTS.md](AGENTS.md).

O computador de continuidade usa Linux. Assets/servidor têm um fluxo nativo;
a DLL do jogo continua sendo Windows x86 e pode ser gerada por compilação
cruzada. Build, testes portáveis, formatação, Clippy e varredura dos assets foram
validados em Fedora 44 com Rust 1.99.0. O build cruzado e a bancada x86 completa
passaram sob Proton Experimental; a integração inicial no jogo também foi
executada. Isso não valida coop entre dois computadores. O roadmap registra
os resultados e limites.
As instruções GNU abaixo descrevem o host Windows
usado nos testes e não devem ser copiadas como configuração nativa Linux.

## Estrutura

```text
Cargo.toml                 workspace com três crates
Cargo.lock                 versões resolvidas das dependências
asset_tools/               CLI de assets e inspeção de arquivos binários
multiplayer_server/        servidor Tokio e biblioteca do protocolo compartilhado
client_dll/                cdylib Win32, leitura local de memória e cliente UDP
  src/bin/client_probe.rs  host Rust que carrega a DLL real para testes
  src/bin/memory_tool.rs   busca/comparação de memória e telemetria externa
  src/bin/mp_launcher.rs   controlador Windows x86 para um PID explícito
  src/bin/proton_host.rs   entrada nativa Linux no prefixo Proton escolhido
  src/bin/test_host.rs     executa a bancada Windows num único runtime
  tests/                  teste entre processos com fixture própria em Rust
```

O cliente depende da biblioteca de protocolo de `multiplayer_server` com as
features do servidor desativadas; não é necessário um quarto crate.

## Git e arquivos do jogo

O `.gitignore` ignora tudo por padrão e libera explicitamente os manifests,
`Cargo.lock`, este README, `ROADMAP.MD`, `AGENTS.md` e os fontes `.rs` dentro dos
três crates, incluindo os testes Rust de `client_dll/tests`. Arquivos do
jogo, extrações, imagens, `.scs`, `.zip`, DLLs, executáveis, logs e `target/`
permanecem locais. Novos tipos de arquivos próprios, como documentação adicional
ou fixtures de testes, precisarão de regras explícitas antes de serem versionados.

Esta pasta está vinculada ao repositório acima, preservando o commit inicial de
licença criado pelo proprietário. Antes de cada commit, confira a lista do staging.
O `.gitignore` não remove
arquivos já rastreados e `git add -f` pode contornar suas regras.

O `.gitattributes` mantém os textos versionados com finais de linha LF para
alternar entre Windows e Linux sem alterações artificiais de formatação.

O código não substitui automaticamente nenhum arquivo da instalação Steam.
Extração e conversão exigem saídas novas; reempacotamento também recusa substituir
arquivos existentes. Uma operação que falhe pode deixar uma saída parcial:
inspecione-a e use outro destino ao tentar novamente.

## Fluxo principal no Fedora

O toolchain atual é **Rust 1.99.0**, fixado em `rust-toolchain.toml`; os registros
Windows com 1.98.1 abaixo são históricos. A migração foi validada por build,
testes nativos e execução dos testes x86 sob Proton. Nenhum toolchain host
Windows é executado nativamente no Linux.

Para reproduzir o preparo local sem acesso administrativo, com Cargo nativo,
`curl`, `dnf download`, `rpm2cpio` e `cpio` disponíveis:

```sh
mkdir -p target/toolchain
curl -fL https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init -o target/toolchain/rustup-init
chmod +x target/toolchain/rustup-init
RUSTUP_HOME="$PWD/target/toolchain/rustup" CARGO_HOME="$PWD/target/toolchain/cargo" \
  RUSTUP_INIT_SKIP_PATH_CHECK=yes target/toolchain/rustup-init -y --no-modify-path \
  --default-toolchain 1.99.0 --profile minimal --component rustfmt --component clippy \
  --target i686-pc-windows-gnu
dnf download --destdir=target/toolchain/rpms mingw32-gcc mingw32-binutils mingw32-crt mingw32-headers mingw32-winpthreads-static
cargo run -p asset_tools --bin dev_env -- unpack target/toolchain/rpms target/toolchain/mingw
cargo run -p asset_tools --bin dev_env -- windows
cargo run -p asset_tools --bin dev_env -- windows --clippy
cargo run -p asset_tools --bin dev_env -- windows --tests
cargo build --workspace --locked
```

`unpack` exige um destino novo e deve receber somente RPMs confiáveis da
distribuição. `dev_env` usa caminhos locais e não sobrescreve configuração Cargo.
Nesta validação: MinGW GCC 16.2.1, binutils 2.46.0 e runtime/headers 13.0.0.
GNU ld emite avisos de aliases stdcall ao exportar nomes sem decoração; a tabela
de exports e as chamadas x86 reais foram verificadas. Não confundir esses avisos
com uma troca da ABI para `extern "C"`.

Prepare um prefixo exclusivo da bancada a partir de uma cópia de um prefixo
Proton já inicializado, com todos os processos de teste anteriores encerrados.
Não use esse prefixo de teste para sua partida. Configure os caminhos locais:

```sh
AA_STEAM="$HOME/.local/share/Steam"
AA_COMPAT="$AA_STEAM/steamapps/compatdata/514160"
AA_PROTON="$AA_STEAM/steamapps/common/Proton - Experimental/proton"
AA_RUNTIME="$AA_STEAM/steamapps/common/SteamLinuxRuntime_4"
# Primeira cópia, com o jogo fechado; preservar também como backup antes de testes.
cp -a --no-clobber "$AA_COMPAT" target/proton-tests
WINEPREFIX="$PWD/target/proton-tests/pfx" "${AA_PROTON%/proton}/files/bin/wineserver" -k
target/debug/proton_host --steam "$AA_STEAM" --compat target/proton-tests \
  --proton "$AA_PROTON" --runtime "$AA_RUNTIME" -- \
  target/i686-pc-windows-gnu/release/test_host.exe target/windows-tests.json
```

`dev_env windows --tests` gera o manifesto dos executáveis de teste retornados
pelo Cargo. `test_host` os executa no mesmo runtime que seu wineserver, inclusive
as fixtures filhas. Nesta máquina, reutilizar um wineserver de outro namespace
do runtime fez a inspeção de fixtures retornar acesso negado. Encerre o wineserver
exclusivo dos testes antes de repetir a bancada em outro runtime; nunca encerre
o wineserver da partida para limpar a bancada.

Para o jogo, abra a instalação original pela Steam, entre numa partida de teste
e localize o **PID Windows** atual (não o PID Linux):

```sh
target/debug/proton_host --steam "$AA_STEAM" --compat "$AA_COMPAT" \
  --proton "$AA_PROTON" --runtime "$AA_RUNTIME" -- \
  target/i686-pc-windows-gnu/release/memory_tool.exe processes
```

Com o servidor nativo iniciado (`cargo run -p multiplayer_server -- --bind
0.0.0.0:39000`), substitua o PID e o endereço pelo destino correto:

```sh
AA_GAME_PID=1234 # substituir pelo PID Windows mostrado acima
target/debug/proton_host --steam "$AA_STEAM" --compat "$AA_COMPAT" \
  --proton "$AA_PROTON" --runtime "$AA_RUNTIME" -- \
  target/i686-pc-windows-gnu/release/mp_launcher.exe \
  --pid "$AA_GAME_PID" --dll target/i686-pc-windows-gnu/release/client_dll.dll \
  connect --server 127.0.0.1:39000
```

`127.0.0.1` serve somente à bancada no mesmo computador. O objetivo do coop é
LAN **entre computadores distintos**; configure o IP LAN do servidor nesses
clientes. Essa matriz entre máquinas permanece pendente.

Na última linha do comando de controle, substitua `connect --server ...` por
`status`, `watch --seconds 30`, `stop` ou `unload`. `unload` primeiro para e
aguarda o worker. Faça `unload` antes de reconstruir/substituir a DLL em uso.
O launcher valida alvo x86, caminho absoluto da DLL e o perfil
`steam-aa-1.10-ccb327c2`; o worker verifica o arquivo do módulo carregado antes
de transmitir. Carga usa `LoadLibraryW`, controle usa `client_control`, descarga
usa `FreeLibrary`, em threads fora de `DllMain`. Os endereços dos exports são
resolvidos nas imagens do alvo, incluindo forwarders, sem presumir endereços
iguais entre processos. Não há alteração de executáveis nem hooks do motor.

O wrapper usa **`runinprefix`**. Não substitua por `run` para ferramentas numa
partida aberta: `run` pode refazer os drives S:/T: sem o ambiente completo da
Steam. Uma chamada inicial assim removeu S: nesta sessão; o link foi restaurado
do backup antes da integração bem-sucedida.

Se uma chamada remota exceder 30 segundos, o launcher retém sua alocação para
evitar liberar dados ainda usados pelo alvo. Ele não termina a thread à força;
encerre/reabra o alvo antes de novas operações de controle nesse caso. Saída
abrupta, menus/reloads e perda de rede prolongada ainda exigem uma matriz maior.

## Ambiente e compilação Windows (registro anterior)

Ambiente encontrado: Windows, Rust **1.98.1**, host `x86_64-pc-windows-gnu`.
Os cabeçalhos PE locais de `aa.exe` e `prism3d.exe` indicam **x86, 32 bits**.
O servidor e a CLI podem rodar em 64 bits; a DLL carregada pelo jogo deve ser de
32 bits. É necessário acesso ao crates.io na primeira compilação.

Na primeira abertura desta instalação Steam, o usuário informou a instalação de
componentes DirectX e do recurso Windows .NET Framework 3.5. A partida foi aberta
com esses componentes presentes; eles fazem parte do registro do ambiente local.

Em um ambiente Rust com linker GNU/MinGW já configurado:

```powershell
cargo build --workspace --locked
rustup target add i686-pc-windows-gnu
cargo build -p client_dll --target i686-pc-windows-gnu --release --locked
```

Saídas relevantes:

```text
target/debug/asset_tools.exe
target/debug/multiplayer_server.exe
target/debug/client_dll.dll
target/debug/client_probe.exe
target/debug/memory_tool.exe
target/i686-pc-windows-gnu/release/client_dll.dll
target/i686-pc-windows-gnu/release/client_probe.exe
```

### Configuração GNU com os componentes distribuídos pelo Rust

Nesta instalação, o linker não estava no PATH e o `dlltool` GNU não tinha um
assembler disponível. O linker GNU de 32 bits também estava sem seu script de
linkedição. O preparo abaixo usa o linker do componente `rust-mingw` em 64 bits,
`rust-lld` em 32 bits e o driver `llvm-dlltool` contido no executável multicall
`llvm-ar` do componente `llvm-tools`. A cópia fica em `target/toolchain/`, ignorada
pelo Git. `.cargo/config.toml` contém os caminhos desta máquina e também é ignorado.

Esse preparo já foi realizado nesta instalação: os comandos normais do Cargo
usam a configuração local. Para reproduzi-lo em outra instalação, use o bloco
abaixo. Se já existir uma configuração Cargo própria, mescle os blocos de target.

Execute na raiz do projeto, em uma sessão PowerShell com o host GNU x64 acima:

```powershell
rustup target add i686-pc-windows-gnu
rustup component add rust-mingw --target x86_64-pc-windows-gnu
rustup component add rust-mingw --target i686-pc-windows-gnu
rustup component add llvm-tools-preview

$aaRustRoot = rustc --print sysroot
$aaNativeBin = Join-Path $aaRustRoot 'lib\rustlib\x86_64-pc-windows-gnu\bin'
$aaGnuBin = Join-Path $aaNativeBin 'self-contained'
$aaToolDir = Join-Path (Get-Location) 'target\toolchain'
New-Item -ItemType Directory -Path $aaToolDir -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $aaNativeBin 'llvm-ar.exe') -Destination (Join-Path $aaToolDir 'llvm-dlltool.exe')
$aaDlltool = Join-Path $aaToolDir 'llvm-dlltool.exe'
$aaGcc = Join-Path $aaGnuBin 'x86_64-w64-mingw32-gcc.exe'
$aaLld = Join-Path $aaNativeBin 'rust-lld.exe'
$aaCargoConfig = @"
[target.x86_64-pc-windows-gnu]
linker = '$aaGcc'
rustflags = ['-C', 'dlltool=$aaDlltool']

[target.i686-pc-windows-gnu]
linker = '$aaLld'
rustflags = ['-C', 'linker-flavor=ld.lld', '-C', 'link-self-contained=yes', '-C', 'dlltool=$aaDlltool']
"@
New-Item -ItemType Directory -Path .cargo -Force | Out-Null
[System.IO.File]::WriteAllText((Join-Path (Get-Location) '.cargo\config.toml'), $aaCargoConfig)

cargo build --workspace --locked
cargo build -p client_dll --target i686-pc-windows-gnu --release --locked
```

A configuração local persiste entre terminais. Ao mover a pasta do projeto,
trocar o toolchain ou executar `cargo clean`, refaça o preparo para atualizar os
caminhos e recriar o driver em `target/`. Para somente executar os programas já
compilados, o preparo não é necessário. Não copie dependências por cima das DLLs
originais.

## Ferramenta de assets

Para investigação estática do motor, `engine_report` é um binário Rust nativo
que lê PE32 e não carrega o módulo:

```sh
cargo run -p asset_tools --bin engine_report -- ../p3shared.dll symbols --contains entity_
cargo run -p asset_tools --bin engine_report -- ../aa/lib/game.dll records
cargo run -p asset_tools --bin engine_report -- ../aa/lib/game.dll disassemble --rva 0xcef70 --length 144
```

Use o caminho real do jogo quando o clone estiver em outra pasta. Relatórios de
originais ficam locais em `target/analysis/`. No Windows/Proton, o comando
`memory_tool.exe entities --pid PID_ATUAL` captura os registros de tipos após
verificar os hashes. Essas ferramentas não criam entidades nem confirmam que um
construtor já possa ser chamado; o roadmap separa descoberta e validação de ABI.

O `data.scs` local tem assinatura ZIP. A ferramenta abre arquivos `.scs` que sejam
ZIP e informa erro para outros formatos; ela não presume que todo SCS seja ZIP.

```powershell
cargo run -p asset_tools -- scan .
cargo run -p asset_tools -- list aa/base.scs --limit 30
cargo run -p asset_tools -- inspect-pe prism3d.exe
cargo run -p asset_tools -- extract aa/base.scs extracted/base --max-mib 2048
```

A extração rejeita caminhos que escapem do destino, nomes especiais do Windows,
links, duplicatas que sobrescreveriam arquivos e arquivos acima do limite total.
Ela também limita a quantidade de entradas. `target`, `.git`, `extracted` e
`generated` são pulados na varredura automática.

Use um caminho de textura obtido com `list` ou na extração:

```powershell
cargo run -p asset_tools -- convert extracted/base/prefabs/train_cross/train_cross.jpg generated/train-cross.png
cargo run -p asset_tools -- convert extracted/base/prefabs/goldengate/goldengate.png generated/goldengate-menor.png --flip-y --max-edge 512
```

Esses dois caminhos foram encontrados no `aa/base.scs` desta instalação.
PNG, JPEG, BMP, TGA e os formatos DDS suportados pelo crate `image` estão
habilitados. DDS não suportado retorna erro; formatos proprietários ainda
precisam de um decoder. O redimensionamento preserva a proporção.

Para reempacotar um diretório preparado localmente:

```powershell
cargo run -p asset_tools -- pack extracted/base generated/base-teste.scs
```

O arquivo resultante usa ZIP sem compressão (`Stored`), com caminhos relativos.
O destino deve ficar fora da árvore de entrada. Links e arquivos que precisariam
de ZIP64 são recusados; pacotes de teste devem ficar abaixo de 4 GiB e 65.535
entradas. A ferramenta não instala o pacote automaticamente na Steam.

A varredura dos três arquivos locais encontrou texturas PNG, JPEG e TGA, sem DDS.
PNG também pode ser um intermediário para edição de outros formatos. Preserve os
nomes e formatos esperados pelos materiais ao montar um pacote. Codificação de
volta para DDS e preservação de seus mipmaps ainda são trabalho futuro, caso
sejam necessárias para novos assets. A compatibilidade final dos pacotes será
verificada em testes dentro do jogo com cópias de segurança.

Para investigar um arquivo binário local sem alterá-lo:

```powershell
cargo run -p asset_tools -- inspect-bytes 'C:\Users\Altenrath\Documents\18 WoS Across America\saves\mp_test.sav' --offset 0 --length 128
cargo run -p asset_tools -- inspect-vectors 'C:\Users\Altenrath\Documents\18 WoS Across America\saves\mp_test.sav' --kind f32 --min-horizontal 100 --max-horizontal 10000000 --max-vertical 100000
```

`inspect-bytes` aceita offsets decimais ou hexadecimais com `0x`, lê até 4096
bytes e tem a opção `--numbers` para interpretar valores little-endian.
`inspect-vectors` examina arquivos de até 16 MiB, com `f32`, `f64` ou `i32`;
suas trincas são hipóteses numéricas, sem conhecimento do formato do salvamento.
O `mp_test.sav` local é binário e não foi reconhecido como ZIP. Nenhuma trinca
é identificada automaticamente como posição do caminhão pelo comando. Com o
intervalo ampliado, a inspeção encontrou duas trincas iguais próximas ao fim do
arquivo, que serviram de referência para investigar a rotina de salvamento.

## Servidor UDP

Teste local, ouvindo somente no loopback:

```powershell
cargo run -p multiplayer_server -- --bind 127.0.0.1:39000
```

Para clientes de outras máquinas da LAN:

```powershell
cargo run -p multiplayer_server -- --bind 0.0.0.0:39000 --max-clients 16 --timeout-secs 5
```

O servidor identifica clientes pelo endereço UDP e atribui IDs. A cada tick de
aproximadamente 1/60 s, retransmite a última posição de cada cliente para os
demais, sem eco para o emissor. Clientes inativos expiram. Há limite de clientes,
de tamanho de datagrama, validação de versão/coordenadas e descarte de sequências
antigas. Ticks perdidos são pulados para evitar rajadas de recuperação.

O protocolo JSON está em `multiplayer_server/src/protocol.rs`. Um envio é:

```json
{"version":1,"sequence":0,"position":{"x":1.0,"y":2.0,"z":3.0}}
```

A retransmissão adiciona `player_id`. O limite é 1200 bytes por datagrama; não há
autenticação, criptografia, garantia de entrega ou sincronização de física.
Este é um protótipo para máquinas de confiança na LAN; 60 Hz é a cadência alvo,
sujeita ao agendamento do sistema e à carga da rede. Para LAN, permita o tráfego
UDP da porta escolhida no firewall, se necessário.

## Testar a DLL real sem o jogo

Com o servidor rodando, abra dois terminais e execute um probe em cada um:

```powershell
.\target\i686-pc-windows-gnu\release\client_probe.exe
```

Cada probe carrega `client_dll.dll` do mesmo diretório, inicia sua thread por uma
função exportada, roda por 10 segundos e encerra a thread antes de descarregar a
DLL. Ele imprime contadores de pacotes e coordenadas dos outros clientes.
O modo padrão envia uma trajetória circular simulada.

Para escolher o destino e diferenciar a trajetória de um dos clientes:

```powershell
$env:AA_MP_SERVER = '127.0.0.1:39000'
$env:AA_MP_MODE = 'simulated'
$env:AA_MP_SIM_PHASE = '3.14159'
.\target\i686-pc-windows-gnu\release\client_probe.exe .\target\i686-pc-windows-gnu\release\client_dll.dll 30
```

Na LAN, substitua o destino pelo IP do computador servidor. As variáveis devem
ser definidas no ambiente do processo que vai carregar a DLL.

## DLL no processo do jogo e coordenadas reais

`DllMain` usa a ABI `extern "system"` Win32 e retorna sucesso com inicialização
mínima. A thread de rede começa em `client_start`, após o carregador retornar.
O host deve chamar `client_stop` e aguardar seu retorno antes de `FreeLibrary`.
Esse ciclo evita inicializar sockets ou aguardar threads dentro do loader lock,
conforme as [orientações da Microsoft](https://learn.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-best-practices).

Exports principais, todos com ABI `extern "system"`:

| Função | Resultado |
| --- | --- |
| `client_control(block) -> u32` | API explícita v1: iniciar, consultar ou parar; 0: sucesso, 1: erro |
| `client_start() -> i32` | 0: iniciado; 1: erro ou já em execução |
| `client_stop() -> i32` | sinaliza e aguarda a thread; 0: sucesso, 1: erro |
| `client_status() -> u32` | 0: nunca iniciado; 1: executando; 2: parado; 3: falha na thread |
| `client_packets_sent() -> u64` | quantidade de envios bem-sucedidos |
| `client_packets_received() -> u64` | quantidade de atualizações remotas aceitas |
| `client_get_remote(index, output) -> i32` | copia um `RemoteState`; 1: encontrado, 0: ausente |

`RemoteState` tem `#[repr(C)]` e está definido em `client_dll/src/lib.rs`. O host
deve fornecer um ponteiro válido e alinhado; o índice percorre os jogadores por
ID. As amostras remotas expiram após cinco segundos sem atualização nova.

O controle novo está em `client_dll/src/api.rs`: `Control` tem 1536 bytes,
alinhamento de oito bytes e `ClientConfig` tem 976 bytes. São estruturas
`repr(C)` sem ponteiros, com versão/tamanho verificados e strings UTF-8
terminadas em NUL. O buffer pertence ao host até a chamada retornar.
`sample_state`: 0 sem amostra, 1 disponível, 2 temporariamente indisponível,
3 falha fatal. Os exports antigos e `AA_MP_*` permanecem para o probe anterior;
`mp_launcher` usa somente a configuração explícita.

Uma cadeia de coordenadas `f32` foi validada nesta instalação, conforme a seção
"Referência validada nesta instalação" abaixo. O modo `memory` exige
`AA_MP_X`, `AA_MP_Y` e `AA_MP_Z`, cada um contendo um endereço de um `f32`, além de
`AA_MP_MODE=memory`. Também são aceitos endereços relativos a módulos e cadeias de
ponteiros na notação descrita abaixo. A leitura usa blocos `unsafe` em torno de
`ReadProcessMemory` no processo atual, verifica o resultado e rejeita NaN/infinito.
Endereços desconhecidos ou ilegíveis não são substituídos por valores inventados.

Os endereços absolutos variam entre sessões. A resolução da base do módulo e da
cadeia funcionou após um reinício nesta versão; isso não garante compatibilidade
com outras versões ou todos os estados de carregamento. As três leituras não
constituem uma captura atômica do estado do jogo.

O probe executa em seu próprio processo: seus endereços não são os do caminhão.
A leitura real exige a DLL no processo correto. As próximas etapas devem
criar e atualizar entidades remotas, interpolar posições e sincronizar rotação,
veículo e estado de sessão. O jogo ainda não oferece multiplayer jogável com
esta base.

## Descoberta das coordenadas com `memory_tool`

Use a versão de **64 bits** em `target/debug/memory_tool.exe` para inspecionar
o jogo de 32 bits. A ferramenta abre o processo somente com permissões de consulta
e leitura. Ela utiliza [VirtualQueryEx](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualqueryex)
para enumerar regiões e [ReadProcessMemory](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-readprocessmemory)
para copiar valores; os comandos não alteram a memória do jogo.

### 1. Localizar o processo e criar uma referência

Abra o jogo, entre em uma partida e deixe o caminhão parado. Localize o PID:

```powershell
.\target\debug\memory_tool.exe processes
```

Nos comandos seguintes, substitua `1234` pelo PID mostrado para `prism3d.exe`:

```powershell
$aaGamePid = 1234
.\target\debug\memory_tool.exe modules --pid $aaGamePid
.\target\debug\memory_tool.exe regions --pid $aaGamePid --limit 30
.\target\debug\memory_tool.exe scan --pid $aaGamePid --output target/memory/01-parado.aamem
```

Por padrão são examinadas regiões comprometidas, legíveis e graváveis. Páginas
protegidas/guardas são ignoradas. A busca considera `f32` alinhados a 4 bytes,
finitos, entre -1.000.000 e 1.000.000 e com módulo pelo menos 0,01. Esses limites
são hipóteses de busca, não informações confirmadas sobre as coordenadas do jogo.

`regions` resume o volume por tipo e mostra as maiores regiões graváveis. A opção
`scan --memory private` limita a busca às alocações privadas; `--memory image`
busca nos executáveis/DLLs carregados; `--memory mapped` seleciona mapeamentos.
O padrão `--memory all` mantém os três tipos. Excluir uma categoria pode remover
variáveis legítimas, portanto as buscas por tipo devem ser tratadas separadamente.

Para procurar valores inicialmente zero, use `--min-abs 0`. Isso pode aumentar
muito a quantidade de candidatos. Para testar coordenadas em precisão dupla,
inicie outra busca com `--kind f64`; alinhamento de 4 bytes também cobre doubles
alinhados a 4 bytes em executáveis x86. `--alignment 1` examina valores sem
alinhamento, com custo maior. `--all-readable` inclui páginas somente de leitura.

A busca padrão limita a leitura selecionada a 512 MiB e os candidatos a quatro
milhões. Ela falha explicitamente ao alcançar um limite, sem salvar uma captura
truncada. `--max-mib`, `--max-candidates` (até oito milhões), `--start` e `--end`
permitem ajustar o escopo. Endereços e offsets são sempre hexadecimais; `--end`
é exclusivo. `--delay-secs 5` dá tempo para retornar ao jogo antes da captura.

Com a partida carregada nesta máquina, a busca geral ultrapassou oito milhões de
candidatos. A referência controlada foi dividida por tipo de memória em
`target/memory/driving-01/`: a busca privada `f32` usou `--min-abs 10` e limite de
oito milhões, enquanto a busca `f64` e as buscas em imagens mantiveram `--min-abs
0.01`. Isso reduz o volume inicial, mas exige investigar à parte possíveis eixos
com módulo inferior a 10, incluindo a coordenada vertical.

Os arquivos `.aamem` contêm PID, caminho do executável, hora de criação do
processo, tipo numérico e pares endereço/valor. Cada arquivo exige um destino
novo e permanece ignorado pelo Git. O carregador verifica tamanho, contagem e
ordem dos registros antes de usá-los. Uma captura pertence a uma sessão do jogo:
após reiniciá-lo, crie uma captura nova. A cadeia relativa validada abaixo pode
ser reutilizada com o novo PID; os endereços absolutos da captura antiga não.

### 2. Filtrar pelo movimento e pelo repouso

Dirija alguns metros, pare e retorne ao terminal:

```powershell
.\target\debug\memory_tool.exe filter target/memory/01-parado.aamem --mode changed --output target/memory/02-moveu.aamem
```

Mantenha o caminhão parado por alguns segundos e filtre novamente:

```powershell
.\target\debug\memory_tool.exe filter target/memory/02-moveu.aamem --mode unchanged --output target/memory/03-parado.aamem
```

Repita deslocamentos e filtros com novos nomes. Os modos `increased` e
`decreased` ajudam a separar eixos quando a direção dos valores já foi observada.
`--epsilon 0.001` define a tolerância absoluta; `--max-delta 100` elimina saltos
acima desse valor. Cada comparação usa o valor da captura imediatamente anterior.
Falhas de leitura descartam os candidatos afetados e são informadas na saída.

```powershell
.\target\debug\memory_tool.exe inspect target/memory/03-parado.aamem --limit 40
```

Para comparar somente os endereços presentes em duas capturas e ver o valor
anterior, o atual e a diferença, use `diff`. Esse comando trabalha com os arquivos
salvos, sem ler o processo em execução. `--group` reúne mudanças numericamente
idênticas para destacar cópias de uma mesma variável:

```powershell
.\target\debug\memory_tool.exe diff target/memory/01-parado.aamem target/memory/03-parado.aamem --min-abs 100 --min-delta 0.01 --max-delta 100 --group --limit 30
```

Os filtros de diferença e módulo do valor são opcionais. Cópias repetidas também
podem pertencer a geometria ou renderização; a frequência ajuda a investigar, mas
não confirma a identidade de uma coordenada.

Um número que muda com o caminhão também pode ser velocidade, câmera, animação
ou uma cópia temporária da posição. Para confirmar um eixo, observe movimentos
em direções diferentes, parada e retorno ao mesmo local. O eixo vertical pode
permanecer constante em terreno plano e desaparecer de um filtro `changed`.
Guarde a captura anterior para investigar esse eixo separadamente.

### 3. Observar valores e resolver ponteiros

Exemplo de sintaxe; os números abaixo não são endereços conhecidos do caminhão:

```powershell
.\target\debug\memory_tool.exe watch --pid $aaGamePid --address '0x12345678' '0x1234567c' --seconds 20 --hz 10
```

A DLL e a ferramenta compartilham a mesma notação de endereços:

| Notação ilustrativa | Operação |
| --- | --- |
| `0x12345678` | lê diretamente nesse endereço |
| `prism3d.exe+0x1234` | usa a base atual do módulo mais o offset |
| `prism3d.exe+0x1234->0x20->0x8` | lê um ponteiro, soma 0x20, lê outro ponteiro e soma 0x8 |

Os offsets são hexadecimais e cada `->` desreferencia um ponteiro antes de somar
o offset. O tamanho do ponteiro acompanha o processo alvo, e há verificações de
endereço nulo, overflow e limite de 16 passos. No observador externo, as bases de
módulos são capturadas no início do comando; reinicie-o se um módulo for recarregado.
Na DLL, a base é consultada a cada amostragem.

O comando `modules` informa bases e tamanhos. Para um endereço realmente situado
em um módulo, o offset é `endereço - base`. Endereços de heap exigem localizar uma
referência/estrutura estável; a ferramenta ainda não descobre cadeias automaticamente.
Uma cadeia aceita pela sintaxe precisa ser validada após reiniciar a partida e o
jogo. A disponibilidade de uma base de módulo não prova a identidade da variável.

`dump` permite examinar até 4096 bytes, exibindo hexadecimal, `u32` e `f32`.
Aceita a mesma sintaxe de ponteiros de `watch` e não escreve no processo:

```powershell
.\target\debug\memory_tool.exe dump --pid $aaGamePid --address 'prism3d.exe+0x1234' --length 64
```

### 4. Transmitir as leituras reais sem carregar a DLL no jogo

Nesta versão local, inicie o servidor e use a cadeia validada com o PID atual:

```powershell
.\target\debug\multiplayer_server.exe
# Em outro terminal; defina $aaGamePid com o PID atual de prism3d.exe:
.\target\debug\memory_tool.exe stream --pid $aaGamePid --x 'game.dll+0x160df0->0x14c->0x4610->0x8->0x8->0x1d0->0x3e4->0x30' --y 'game.dll+0x160df0->0x14c->0x4610->0x8->0x8->0x1d0->0x3e4->0x34' --z 'game.dll+0x160df0->0x14c->0x4610->0x8->0x8->0x1d0->0x3e4->0x38' --server 127.0.0.1:39000 --seconds 30
```

Esse cliente externo lê o processo e usa o mesmo protocolo UDP da DLL, com
cadência alvo de 60 Hz. Ele também consome e conta atualizações de outros clientes.
`--kind f64` permite ler doubles e convertê-los para os `f32` do protocolo, desde
que o resultado seja finito. A DLL atualmente lê coordenadas `f32`.

O comando `fixture` executa um processo controlado inteiramente em Rust, imprime
endereços conhecidos e aceita `set X Y Z` pela entrada padrão. Ele serve para
testar a busca e o envio entre processos sem depender dos endereços do jogo.

### Referência validada nesta instalação

A inspeção local da rotina de salvamento apontou para esta cadeia de posição:

```text
game.dll+0x160df0->0x14c->0x4610->0x8->0x8->0x1d0->0x3e4
```

Acrescente `->0x30` para X, `->0x34` para Y e `->0x38` para Z. A cadeia usa a
base atual de `game.dll`; não use os endereços absolutos da sessão como constantes.
Os offsets foram investigados somente no arquivo local com SHA-256
`ccb327c2a5c2f7d01d74d94629cf65847ee1535ca9020b3a5fc5d412a8ffbc7d`.
A cadeia foi validada após fechar o jogo, abri-lo novamente e carregar
`mp_test2`. Ainda não foi verificada em outras versões. A escala dessas unidades
ainda não foi medida.

Na mesma sessão, antes e depois de um avanço com curva e colisão relatado pelo
usuário, as leituras foram:

| Captura | X | Y | Z |
| --- | ---: | ---: | ---: |
| Antes do deslocamento | 1113864.875000 | 23570.652344 | 794032.062500 |
| Após o avanço e a colisão | 1108120.125000 | 23595.759766 | 793902.875000 |

A orientação na mesma estrutura também mudou. Os três valores ficaram estáveis
durante a observação de três segundos após a colisão. Uma leitura posterior
registrou outro deslocamento, preservado em
`target/memory/driving-01/after-collision-position.aamem`. Esse comportamento
reforça a identificação, mas não determina a escala em metros. Não se presume
que toda mudança em Y seja causada pela colisão.

Após um novo deslocamento em zigue-zague, o usuário salvou `mp_test2`, encerrou o
jogo e carregou esse salvamento em um novo processo. A base de `game.dll` mudou
de `0x04e10000` para `0x04d70000`; o endereço final de X mudou de `0x13a1fc58`
para `0x1307f818`. Sem alterar nenhum offset da cadeia, a leitura retornou:

```text
X = 1107795.750000000
Y =   23592.041015625
Z =  798043.312500000
```

Os três `f32` coincidiram exatamente com os valores em `mp_test2.sav`, a partir
do offset de arquivo `0x1ec33`. Os quatro componentes da orientação também
coincidiram. A captura dessa sessão está em
`target/memory/driving-01/restarted-mp-test2-position.aamem`. Esse offset do
salvamento é uma observação dos dois arquivos locais de mesmo tamanho, não um
formato universal documentado. Os salvamentos foram somente lidos.

```powershell
.\target\debug\memory_tool.exe watch --pid $aaGamePid --address 'game.dll+0x160df0->0x14c->0x4610->0x8->0x8->0x1d0->0x3e4->0x30' 'game.dll+0x160df0->0x14c->0x4610->0x8->0x8->0x1d0->0x3e4->0x34' 'game.dll+0x160df0->0x14c->0x4610->0x8->0x8->0x1d0->0x3e4->0x38' --seconds 20
```

As três leituras foram transmitidas pelo cliente externo para o servidor local:
240 pacotes enviados em quatro segundos. Uma segunda instância, com a DLL de
32 bits carregada em `client_probe`, recebeu esses valores. Esse teste confirma
o caminho leitura → UDP → servidor → DLL receptora. A DLL continua com endereços
configuráveis; a referência local é fornecida explicitamente pelo host.
O teste foi repetido após o reinício: 180 pacotes em três segundos, com os
valores de `mp_test2` recebidos pelo `client_probe` de 32 bits. Os processos de
teste foram encerrados ao final; o processo do jogo permaneceu aberto.

## Verificação

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo test -p client_dll --target i686-pc-windows-gnu --locked
```

Os testes usam conteúdo próprio temporário e sockets locais. Cobrem extração e
reempacotamento, recusa de sobrescrita/traversal, limites de extração, conversão
de imagem, protocolo, retransmissão entre dois clientes e leitura de valores
locais válidos/inválidos. Nenhum teste altera os assets originais.

O teste `client_dll/tests/memory_workflow.rs` inicia uma fixture Rust em outro
processo, encontra seus valores, aplica filtros, resolve uma cadeia de ponteiros,
lê `f64` e recebe via UDP as coordenadas efetivamente lidas. Ele também verifica
a recusa de capturas de outra sessão e a preservação de saídas já existentes.

Registro anterior de validação no Windows (antes da integração Fedora/Proton):

- Quatorze testes do workspace passaram em 64 bits e sete testes do cliente passaram
  em 32 bits; Clippy passou com avisos tratados como erros.
- Os três `.scs` originais foram abertos pela CLI. `data.scs` foi extraído em
  `target/smoke/`, uma textura TGA foi convertida para PNG e os 76 arquivos foram
  reempacotados em um arquivo de teste separado.
- Dois processos carregaram a DLL de 32 bits e receberam coordenadas simuladas
  um do outro pelo servidor local.
- A ferramenta abriu o `prism3d.exe` de 32 bits, capturou e comparou memória
  durante movimentos. As capturas numéricas iniciais não identificavam sozinhas
  o caminhão; a identificação veio da rotina de salvamento e da cadeia validada.
  Os arquivos em `target/memory/` e `target/analysis/` ficam ignorados pelo Git.
- A investigação encontrou posições com componentes acima de um milhão; o
  intervalo padrão da busca não representa o limite conhecido do mundo. Filtros
  estritos de repouso também podem eliminar pequenas variações da suspensão.
- Os salvamentos `mp_test.sav` e `mp_test2.sav` foram apenas lidos. A cadeia acima
  foi localizada seguindo a rotina que grava a posição e validada após reinício:
  posição e orientação coincidiram exatamente com `mp_test2`. Houve recepção
  das leituras da sessão anterior por uma DLL de 32 bits em outro processo,
  via servidor local.
- Os arquivos originais, as saídas de teste, os binários e a configuração local
  do compilador permanecem excluídos pelas regras do Git.

O pacote reempacotado ainda precisa de teste dentro do jogo. A integração inicial
da DLL foi validada posteriormente no Fedora/Proton: 12 testes nativos e 11 testes
Windows x86 passaram, além de leitura/envio/recepção reais no jogo, movimento,
menu/outra partida, parada e descarga. O envio suspendeu durante a cadeia
indisponível e retomou automaticamente nos novos endereços. Caminhões remotos
e teste LAN entre computadores continuam pendentes; consulte o roadmap para
evidência e limitações.
