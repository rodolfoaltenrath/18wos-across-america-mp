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
A criação e destruição imediata de um `actors.car` privado também passaram,
incluindo repetição após recarregar uma partida.
No Windows, o proprietário já confirmou um caminhão de teste sem textura,
4 m ao lado do seu, copiando seus movimentos e rotação. É um visual PSM,
sem física/colisão próprias, não outro jogador conectado.
Os avanços seguintes têm provas na bancada própria; a última verificação
registrada passou com 41 testes x86 e 47 nativos, Clippy e formatação.
O eco num PC já transmitiu/desenhou poses com rotação na partida; após fornecer
iluminação da cena, o usuário confirmou a textura correta durante movimento.
Carregar com liberação prévia e recriar explicitamente também passou no jogo.
Recarga automática com instância retida, vários remotos e LAN entre dois PCs
continuam sem validação. Ainda não há multiplayer LAN validado.
O primeiro teste com eco já desenhou poses recebidas, mas o usuário relatou
material preto e atraso; carregar um save com o visual retido fechou o jogo.
A recarga automática permanece sem validação real. Ver o procedimento abaixo:
liberar o visual antes do carregamento, até resolver o lifetime na transição.

O plano completo, os critérios de conclusão e o procedimento para continuar em
outro computador estão em [ROADMAP.MD](ROADMAP.MD). As orientações de autonomia
e continuidade do desenvolvimento estão em [AGENTS.md](AGENTS.md).

## Direção do mod definida pelo proprietário

O mod oferecerá somente a carreira baseada em **Become a Millionaire**, com
competição econômica entre transportadoras. Antes de criar um save, será
possível definir a meta financeira ou marcar **Sem limite**. Com essa opção,
a carreira continua sem vitória/encerramento pelo valor acumulado; dinheiro,
despesas e progressão continuam funcionando. Não será um segundo modo Free
Mode. A regra escolhida acompanhará o save e será comum à sessão multiplayer.
Essa configuração e a alteração dos menus/regras ainda não foram implementadas.

A evolução aprovada inclui empresas dos jogadores, fretes compartilhados com
aceite exclusivo, pagamentos/despesas validados pelo servidor, frota e ranking.
Motoristas IA, licitações e cooperativas ficam para depois da prova de um
contrato completo, sem pagamento duplicado, inclusive após desconexão/reinício.
A economia original permanece local durante a validação do MVP visual.

A prioridade é **LAN**. Hospedagem externa é uma possibilidade futura: manter
o servidor Rust independente do processo do jogo e da interface, com endereço
configurável e separação entre conexão, identidade, regras e persistência.
Contratar VPS e preparar operação pública não fazem parte da etapa atual.

Uma etapa futura traduzirá o jogo para **PT-BR** e adotará **R$ (reais)** na
moeda do jogo. Isso inclui revisar textos, fontes/acentos e apresentação de
valores; conversão/rebalanceamento dos preços ainda precisam de definição.
Não haverá dependência de cotação cambial real. O roadmap separa essas metas
do que já foi entregue e registra as dependências técnicas.

O proprietário retornou ao Windows em 28/10/2026; os avanços feitos no Fedora
foram preservados. Build, bancada x64/x86, transmissão da partida e recuperação
após carregar um save também passaram no Windows nativo. Consulte o fluxo
Windows abaixo e o registro de retomada no roadmap. No Linux, assets/servidor têm um fluxo nativo;
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
  src/bin/engine_fixture.rs janela Rust ANSI/Unicode para testar o dispatcher
  src/bin/fixture_actor/   factories e métodos virtuais próprios da bancada x86
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
iguais entre processos. Não há alteração de executáveis. A extensão opcional de
diagnóstico abaixo instala temporariamente um procedimento na janela; não
intercepta instruções do motor.

O wrapper usa **`runinprefix`**. Não substitua por `run` para ferramentas numa
partida aberta: `run` pode refazer os drives S:/T: sem o ambiente completo da
Steam. Uma chamada inicial assim removeu S: nesta sessão; o link foi restaurado
do backup antes da integração bem-sucedida.

Se uma chamada remota exceder 30 segundos, o launcher retém sua alocação para
evitar liberar dados ainda usados pelo alvo. Ele não termina a thread à força;
encerre/reabra o alvo antes de novas operações de controle nesse caso. Saída
abrupta, menus/reloads e perda de rede prolongada ainda exigem uma matriz maior.

Para testar o dispatcher de diagnóstico, com a partida aberta, use o mesmo
comando `proton_host ... -- mp_launcher.exe --pid ... --dll ...`, substituindo
o comando final por cada uma destas ações, em sequência:

```text
engine attach
engine probe
engine registry
engine inspect actors.car
engine detach
unload
```

`attach` carrega a DLL e seleciona a maior janela visível sem proprietário do
processo; `engine attach --window HANDLE_DECIMAL` permite escolher explicitamente.
As outras ações exigem a DLL já carregada. `probe` informa a thread da janela e
a thread que executou o comando; `registry` verifica os hashes dos dois módulos
e conta os tipos; `inspect` retorna tamanho, construtor e destrutor da variante
`false`, sem executá-los. Esta extensão usa `EngineControl` v3 de 664 bytes e
preserva a ABI v1 de configuração/rede. Reconstrua launcher e DLL juntos; blocos
antigos da extensão são recusados por versão/tamanho. Esses comandos de consulta
não criam nem movem objetos.

A fila tem limite de 16 comandos, identificadores sem reutilização e pedidos
pertencentes à DLL. O timeout de 1 segundo cancela trabalho ainda não iniciado;
um comando que já começou pode terminar depois. A DLL mantém uma referência
adicional enquanto o procedimento estiver instalado. `detach` restaura o
procedimento original na thread da janela e só libera essa referência após
confirmar o retorno da chamada. `stop`/`unload` também fazem essa limpeza; se
outro componente substituir o procedimento ou a janela deixar de responder,
a descarga falha e a referência permanece até uma remoção segura ou o fim do
processo. Mensagens normais continuam encaminhadas ao procedimento original.

O ciclo passou em janela Rust própria ANSI/Unicode e numa partida sob Proton,
com 80 registros consultados. Uma primeira tentativa perdeu o processo antes
da primeira consulta; a causa não foi determinada. A repetição passou, mas a
estabilidade prolongada e a fase do motor continuam pendentes. Executar na thread
da janela, por si só, não autoriza chamar construtores do jogo. Consulte o roadmap.

Para investigar o ponto do loop principal, a extensão também oferece:

```text
engine attach
engine trace
engine pump-attach
engine pump-status
engine pump-remove
stop
```

`trace` captura até 32 frames; sob o Proton desta instalação o unwind terminou
no despachante Win32 sem alcançar o executável, portanto não comprova a fase.
`pump-attach` verifica o executável e os dois módulos por SHA-256, exige a entrada
original de `PeekMessageA` e instala uma sonda somente na entrada IAT do perfil.
O shim stdcall é escrito em Rust com `naked_asm!`; preserva os cinco argumentos,
o retorno e o erro Win32, e captura o endereço de retorno real do chamador.
Ele conta retornos dos três pontos identificados do loop somente na thread da
janela. Instalar a sonda não cria entidades; operações do motor exigem o comando
explícito abaixo.

**Usar `pump-attach` fixa a DLL na memória até encerrar o jogo.** Remover uma
entrada IAT não prova que outra thread não tenha carregado seu valor anterior.
`pump-remove` e `stop` restauram a entrada e a proteção da página; `stop` também
remove o procedimento da janela. `unload` informa que a DLL continua residente
e recusa novas chamadas a `FreeLibrary`. Encerre o jogo antes de reconstruir essa
DLL. A fixação só ocorre nessa sonda opcional; o ciclo normal do dispatcher
continua permitindo descarga completa. Estados: 0 nunca instalada, 1 instalada,
2 restaurada com módulo residente. A sonda não pode ser reinstalada no mesmo
processo. Qualquer entrada já modificada é recusada.

Na partida sob Proton foram observados 3.414.859 retornos do loop. O proprietário
dirigiu, voltou ao menu e recarregou o save sem travamentos. A entrada original
foi restaurada, os contadores pararam e o procedimento da janela foi removido;
o módulo permaneceu residente conforme previsto. Passaram 16 testes nativos e
14 testes x86. Isso ainda não valida criação/remoção de objetos do motor.

Referências da implementação: [assembly em Rust](https://doc.rust-lang.org/reference/inline-assembly.html),
[proteção de páginas Win32](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualprotect)
e [referências ao módulo e fixação](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-getmodulehandleexw).

Para validar o lifetime de uma entidade privada, com o caminhão local parado
num save de teste e o perfil compatível:

```text
engine attach
engine pump-attach
engine phase-probe
engine private-cycle
engine pump-remove
stop
```

`phase-probe` enfileira uma operação até o retorno observado de `PeekMessageA`.
`private-cycle` cria um único `actors.car` não inicializado pelo factory do motor
e o destrói na mesma operação. `--cycles N` permite 1 a 16 ciclos, sempre um
objeto por vez. Não insere no mundo, não carrega um modelo e não cria um caminhão
visível. Os endereços e o tipo são fixados pelo perfil; o comando do jogo não
aceita callbacks ou ponteiros arbitrários.

A fila do loop é separada da fila de janela, limita-se a 16 pedidos e só executa
um por passagem, na thread esperada, fora de callbacks da janela e de chamadas
aninhadas de `PeekMessageA`. O pedido é uma cópia pertencente à DLL. Timeout antes
do início cancela a operação; timeout depois do início não interrompe nem libera
seus dados. Nesse caso, consulte `pump-status` e aguarde `running=0` antes de
decidir repetir. A parada cancela pendências e recusa desmontagem durante trabalho
ativo. O worker UDP não pode criar entidades por essa interface.

Os factories usam `cdecl`; os métodos virtuais internos usam `thiscall`. A criação
seleciona a variante `false` com armazenamento nulo, delegando alocação ao motor.
Um guard Rust devolve o objeto exclusivamente a `entity_destroy`, inclusive após
falha de validação Rust. O objeto não é copiado nem liberado pelo alocador Rust.
São conferidos vtable, destrutor e contadores dos contextos do motor e do jogo,
incluindo ancestrais compartilhados. A posição local precisa continuar igual
durante o comando. Esses controles não capturam exceções nativas C++/SEH.

A bancada usa factories e métodos virtuais escritos em Rust num processo próprio,
com rastreamento de alocações, contadores e thread. Valida o caminho completo entre
DLL e host sem usar assets comerciais. A evidência do teste real e os limites
ficam no roadmap. Encerre o jogo antes de reconstruir a DLL fixada pela sonda.
Passaram 19 testes nativos e 17 testes x86. No jogo, 18 ciclos privados passaram,
incluindo repetição após dirigir, salvar manualmente e recarregar. Contadores e
posição local foram conferidos em cada comando, e a instrumentação foi removida.

O comando `engine item-cycle [--cycles N]` valida um segundo contrato: um
`actors.car` privado construído na memória de um `c_item_t` anônimo. Usa a mesma
preparação `attach`/`pump-attach`, fila do loop e limpeza `pump-remove`/`stop`.
Limita-se a 1–16 ciclos e não inicializa nem insere o objeto no mundo. O item
fornece armazenamento e mantém a referência inicial; a DLL solta sua fixação e
devolve essa referência. As operações do próprio item chamam o destrutor com
armazenamento explícito e liberam a memória. Rust não libera esses ponteiros.
São conferidos a tabela de operações, cabeçalhos, referências, retorno ao pool,
bytes alocados/liberados e posição local. Esse caminho também passou 18 vezes
no jogo, incluindo movimento/menu/recarga, sem mudanças no save ou nos originais.
Os testes x86 cobrem falhas antes e depois da construção e cancelamento pendente.
Se a fixação perder sua exclusividade, a limpeza retém o item e informa falha,
em vez de liberar armazenamento de propriedade incerta. Isso não cobre exceções
C++/SEH nem constitui auditoria completa do heap.

Referências de ABI: [funções externas em Rust](https://doc.rust-lang.org/reference/items/functions.html#extern-function-qualifier)
e [convenções de chamada x86 da Microsoft](https://learn.microsoft.com/en-us/cpp/cpp/argument-passing-and-naming-conventions?view=msvc-170).

Para consultar os metadados sem carregar a DLL, execute pelo mesmo `proton_host`
o comando `memory_tool.exe values --pid PID_ATUAL`. A ferramenta confere os hashes
de `game.dll`/`p3shared.dll` e decodifica a cadeia de `evalue_def_t` de `actors.car`,
incluindo as definições herdadas. Retorna nomes, campos/offsets assinados,
callbacks e as 24 palavras originais por nó. Não executa getters/setters.
Há limites de 2048 nós e 256 bytes por nome, detecção de ciclos, overflow,
memória ilegível e mudança dos links durante a captura.

Na partida foram encontradas sete propriedades. `truck_build` é uma string
de capacidade 1024 em `+0x12c8`, mas estava vazia no caminhão carregado;
`position`/`rotation` são triplas declaradas em `+0x94`/`+0xe0`, cujos valores
não comprovam o transform atual da física. Não usar esses campos diretamente
como coordenadas/orientação de rede. O relatório é observacional, específico
do perfil; não define uma ABI de escrita nem uma captura atômica do processo.

A ferramenta nativa também permite ler uma definição sem extrair o SCS:

```sh
cargo run -p asset_tools --bin asset_tools -- read ../aa/base.scs makes/truck/truck_base.def
cargo run -p asset_tools --bin engine_report -- ../aa/lib/game.dll string --rva 0x14dba4
```

`read` exige o nome exato, UTF-8 e limite de 64 KiB por padrão (`--max-kib`,
máximo 1024). Confere tamanho declarado e efetivo, rejeita caminhos inválidos,
entradas duplicadas e links/arquivos especiais. Nenhum arquivo é extraído.
Relatórios com conteúdo comercial permanecem locais, fora do Git.

O candidato visual `decals.pmd` também tem um perfil de leitura e lifetime:
`memory_tool.exe values --pid PID --entity decals.pmd` e
`mp_launcher.exe ... engine item-cycle --kind pmd`. Este último continua criando
somente um objeto privado não inicializado, destruído na mesma operação. Foram
validados 17 ciclos reais e 13 propriedades, incluindo `model_name`,
`model_skins`, `model_scale`, `model_rotation` e `play_animation`. Isso ainda não
comprova carregamento do modelo nem presença/ausência de colisão após init.
O formato JSON expõe `element_size` e `table_bound`: o segundo é capacidade de
array inline ou parâmetro de crescimento, conforme o tipo de tabela; não é
um offset no objeto. Os campos brutos continuam disponíveis.

Para testar somente a configuração do nome de modelo no PMD privado, use
a mesma preparação `engine attach` / `engine pump-attach`, seguida de:

```text
engine phase-probe
engine model-cycle --model models/ai/truck/models/truck.psm
engine model-cycle --model models/ai/truck/models/truck.psm --cycles 16
engine pump-remove
stop
```

`model-cycle` fixa o tipo `decals.pmd`, cria um item, escreve `model_name` pelo
export cdecl do motor, confirma os bytes e libera o item na mesma operação.
O campo deve estar inicialmente vazio; nada fora de seus 64 bytes pode mudar.
São aceitos somente caminhos ASCII relativos `models/*.psm`, sem componentes
vazios, `.`/`..`, barras invertidas ou NUL, com até 63 bytes mais terminador.
O caminho é uma string de configuração: esta prova não abre o asset, carrega
recursos, inicializa render/colisão ou insere um caminhão no mundo.

O perfil exige tipo de metadado **21**, flags 0, offset `0x200`, elemento 64,
limite/auxiliar/contagem `-1` e o setter exportado em `p3shared.dll+0x1b10`.
String genérica de tipo 14 é recusada mesmo que compartilhe o callback.
A definição é relida antes de cada chamada; alteração de metadados, retorno
false, escrita fora do campo e confirmação divergente falham com limpeza pelo
item. A extensão continua ABI v3/664 bytes, com novos comandos 17/18; `name`
transporta o caminho nesses comandos. Nenhum ponteiro é aceito na CLI do jogo.
Após a limpeza, feche normalmente o processo fixado antes de reconstruir a DLL.
Passaram 18 ciclos reais de setter no Windows, incluindo repetição após dirigir
e recarregar. Os slots de carga/liberação PMD estavam nulos nessa partida;
resolver essa dependência é necessário antes de habilitar init/render.

Para investigar acessos candidatos a membros, sem executar uma imagem:
`engine_report ARQUIVO field --offset 0x200`. A busca distingue memória baseada
em registrador de imediatos/endereços absolutos, mas não prova o tipo do
registrador nem se a instrução é alcançada.

Para investigar a resolução dos recursos sem carregar a DLL do projeto:

```text
engine_report aa/lib/game.dll pointers --rva 0x160e20
engine_report aa/lib/game.dll bindings --rva 0xce970 --length 4096 --contains psm_
memory_tool resources --pid PID_ATUAL
```

`pointers` encontra os quatro bytes de um VA preferido, inclusive sem alinhamento
e em dados; retorna o RVA do início desses bytes, não o início de uma instrução.
O limite de ocorrências falha explicitamente em vez de truncar o resultado.
`bindings` parte de uma fronteira de instrução fornecida e reconhece somente
uma rotina linear `GetProcAddress` → armazenamento de EAX em slot global.
Reconhece a escrita atrasada após os próximos argumentos e a restauração do
registrador do resolver antes da última escrita. Retornos, branches, chamadas
desconhecidas e operações não suportadas encerram a análise. Não é um grafo de
fluxo completo nem prova de que uma rotina foi executada ou de sua ABI.

`resources` verifica os hashes de executável, engine, jogo e do `psm.dll` quando
carregado. Compara seis slots PSM com RVAs perfilados e os exports nomeados do
módulo remoto, relê os slots e confere se os módulos continuam iguais. O JSON
distingue `matched_export`, `unbound`, `unexpected_target`, `unprofiled_target`
e `provider_missing`. `psm_exports_match` resume somente as seis comparações;
não autoriza chamadas. Os rótulos PMD são candidatos, sem assinatura atribuída.
O comando apenas lê memória; não injeta, não instala sonda e não exige foco na
janela. Uma captura aceita não é um snapshot atômico do processo.

O módulo PSM tem hash explícito no perfil. Relatórios completos e dados de
endereços da sessão devem permanecer locais em `target/analysis/`, fora do Git.

Para testar a carga privada do descritor de um modelo PSM, prepare o mesmo
dispatcher/sonda (`engine attach`, `engine pump-attach`) e execute:

```text
engine phase-probe
engine info-cycle --model models/ai/truck/models/truck.psm
engine info-cycle --model models/ai/truck/models/truck.psm --cycles 16
engine pump-remove
stop
```

`info-cycle` usa `psm_alloc_info`/`psm_info_release` por cdecl na fila do loop.
Confere os quatro hashes, os seis bindings PSM e quatro callbacks do provedor
para recursos/alocador do núcleo. Aceita o mesmo caminho ASCII `models/*.psm`
de até 63 bytes, com partida carregada. Confere descritor de 16 bytes, referência
inicial 1, item de modelo, ausência de animações e identidade do contexto PSM.
A liberação cabe ao provedor, inclusive quando uma verificação Rust falha;
nenhum ponteiro do motor é liberado pelo alocador Rust.

A referência forte do item está em `c_item_t+0x4c`. Após liberar o descritor,
o comando tenta reler esse contador pela API de memória do sistema, sem
dereferenciar o item em Rust nem assumir que ainda o possui. Memória ilegível
ou valor divergente torna a prova inconclusiva/falha; não provoca nova liberação.
O contador observado do contexto é direto (`+8`), sem prova de toda a árvore
de módulos ou contabilização de todos os bytes do heap. A primeira repetição
pode reter referências de recursos; crescimento nas repetições seguintes da
mesma chamada é recusado após liberar o descritor. `retained module delta`
informa esse saldo, sem classificá-lo automaticamente como cache legítimo.

Passaram 18 ciclos no Windows, incluindo recarga: a primeira carga observou
`922/924/923` referências de contexto; as 16 seguintes, `923/924/923`; após
dirigir e carregar o save, `903/904/903`. As referências do item balancearam e
a posição local não mudou dentro das operações. O saldo inicial de uma
referência permanece registrado para investigação. Nenhum descritor fica vivo
entre frames; o ciclo de instância descrito abaixo amplia essa prova. Desenho,
init de PMD e veículo remoto continuam pendentes. ABI de diagnóstico v3/664
bytes preservada, com comandos 19/20;
configuração/rede v1 preservada. Após a limpeza, feche o processo normalmente
antes de reconstruir a DLL fixada pela sonda.

O passo seguinte tem um diagnóstico separado de instância privada:

```text
engine phase-probe
engine instance-cycle --model models/ai/truck/models/truck.psm
engine instance-cycle --model models/ai/truck/models/truck.psm --cycles 16
engine pump-remove
stop
```

Prepare `engine attach` e `engine pump-attach` antes desses comandos. Esta prova
é restrita ao caminho indicado, com capacidade 1 e nenhuma skin fornecida.
O motor usa seu shader de fallback `notfound.shd`; aparência/texturas corretas
exigem uma etapa posterior. A operação cria um único descritor para a chamada,
mantém sua referência e passa `(info, NULL, 0, 1)` ao `psm_alloc_inst` cdecl.
Cada instância retorna como `c_item_t`, e `psm_free` recebe esse item, nunca
o ponteiro de dados em `item+8`. A instância é liberada antes da próxima e o
descritor é liberado ao final, também nas saídas de erro Rust.

São verificados os bindings de recursos, exports, callbacks de alocação/pacote,
tabela de operações, referência do descritor `1→2→1`, referências do modelo,
referências fortes/residência/acesso do item `1/1/0`, cabeçalho e limites dos
arrays. Referências de shaders repetidos são agregadas antes de conferir sua
liberação. O contador forte do item devolvido ao pool é relido pela API do
sistema, sem dereferência Rust. Alterações no perfil ou contadores divergentes
interrompem a prova; retenção inicial é informada, crescimento repetido falha.
Os contadores e tamanho retornados referem-se ao último ciclo, com o info ainda
retido durante a medição. Isso não comprova liberação de todo o heap/cache.

Não há draw, colisão, HUD, inserção no mundo ou retenção da instância entre
frames. O ponto observado no loop ainda não é uma fase de render comprovada.
A extensão usa comandos 21/22 sem mudar a ABI v3/664 bytes. A bancada própria
exercita falhas, shader compartilhado, retorno ao pool e timeout sem execução
tardia; consulte o roadmap para os resultados da partida real.
Passaram 18 ciclos reais de instância no Windows, incluindo repetição após
dirigir/menu/recarga, com bloco de dados de 26.268 bytes e contadores estáveis
nas repetições. Os módulos originais e os dois saves conservaram seus hashes;
a sonda/dispatcher foram removidos e o jogo encerrado normalmente.

Para observar as chamadas gráficas existentes, sem pedir novos desenhos:

```text
engine attach
engine pump-attach
engine render-attach
engine render-arm
engine render-status
engine render-report
engine render-remove
engine pump-remove
stop
```

Deixe a partida renderizar um flush completo entre `render-arm` e
`render-report`. O relatório retorna JSON; mantenha-o local em
`target/analysis/`. Se a captura ainda estiver armada/em andamento, o comando
pede uma nova consulta. `render-status` retorna somente estado/contadores em
JSON, inclusive com captura pendente, sem rearmar ou ler os registros.
Mantenha a partida visível em primeiro plano: nesta instalação o loop continuou
ativo sem chamadas de render/flush enquanto a janela estava sem foco; voltar
à partida permitiu concluir a captura já armada.
Pode usar `render-arm` novamente para outra captura,
com geração nova; a instalação da sonda só pode ocorrer uma vez por processo.
`stop` também restaura os slots gráficos antes de remover o dispatcher.
O processo permanece com a DLL fixada até encerrar, pela regra da sonda do loop.

A observação intercepta o import thiscall `r_device_t::render_pool` no núcleo
e o binding cdecl `gfx_path_flush` no jogo, conferindo hashes e exports antes
de alterar os dois ponteiros. Encaminha cada chamada original uma vez, preserva
`LastError` na entrada/saída e apenas lê dados para diagnóstico. Não invoca
PSM/draw, não insere entidades e não muda filas ou matrizes do motor.

Cada captura acompanha um flush externo na thread da janela: entrada, pools
antes/depois de render, pools após retornar do flush e retorno final. Guarda
até 128 registros e 16 pools distintos, com orçamento de 256 nós por leitura
da fila. `prototype_prefix`/`bucket_prefix` são contagens da parte lida. Só
`queue_readable=true` sem `truncated` permite interpretar `empty_observed`.
Uma leitura inválida/cíclica ou truncada nunca é apresentada como fila vazia.

O `summary.flags` deve ser zero para uma captura completa: bits 1/2 indicam
armada/em andamento; 4/8 indicam limite de registros/pools; 16 indica flush
aninhado; 32 indica perda por disputa do buffer. Chamadas de outras threads
continuam sendo encaminhadas, mas são somente contadas em `foreign_calls`.
`generation` impede misturar registros de capturas diferentes.

As matrizes são cópias de 16 `f32` dos ponteiros de `gfx_world_matrix` e
`gfx_camera_matrix`, sem normalização ou interpretação de eixos/origem. Leitura
inválida ou valores não finitos aparecem como `null`. O estado pode representar
um passe de interface ou outro contexto; ainda não é um contrato de câmera 3D.
Fila CPU observada vazia não comprova conclusão da GPU, outras filas/passes
ou autorização para liberar imediatamente uma instância desenhada.

O perfil acrescenta o hash de `gfxshared.dll`. A ABI de diagnóstico continua
v3/664 bytes, comandos 23–28; nos registros gráficos `frames` transporta bits
das matrizes, com codificação em `render_observation`, sem mudar comandos
anteriores. Configuração/rede v1 preservadas. A bancada gráfica é própria,
sem assets comerciais; os resultados no jogo ficam registrados no roadmap.

No Windows, a sonda passou em três capturas reais: parado, após dirigir/fazer
curva e após menu/recarga. Foram 24 registros, sem perda, na thread esperada;
duas filas mantiveram 9/1 itens durante render e ficaram vazias após flush.
Os slots originais foram restaurados e o processo encerrou normalmente, sem
problemas relatados. As capturas observaram o mesmo passe (`game.dll+0x800f2`),
sem provar todos os passes ou a convenção das matrizes. Ainda não há desenho
de caminhões remotos.

### Ponto de submissão 3D: sonda passiva do `draw` do caminhão

A análise estática (roadmap, seção "Passe 3D e ponto de submissão") mostrou
que o flush observado acima pertence ao `draw` de uma entidade de céu/sol, e
que caminhões entram na fila pelo `entity_t::draw` virtual de `actors.car`.
Para comprovar esse ponto na partida, sem desenhar nada:

```text
engine attach
engine pump-attach
engine render-attach
engine draw-attach
engine draw-arm
engine draw-status
engine draw-report
engine draw-remove
engine render-remove
engine pump-remove
stop
```

`draw-attach` substitui somente o slot `+0x6c` da vtable de `actors.car`
(`game.dll+0x12580c`, original `+0x5a6a0`), depois de conferir hashes de
núcleo/jogo e o valor original. O shim thiscall encaminha a chamada uma vez,
preserva `LastError` e apenas lê memória. Cada captura registra 16 pares
entrada/retorno (32 registros) na thread da janela, com `this`, vtable, byte
de estado `+0x1e9`, retorno do chamador, profundidade da pilha world do núcleo,
path/informação/viewport correntes, quaternion/posição do corpo físico
(`[[this+0x1d0]+0x3e4]+0x14/+0x30`) e cópias das matrizes world/camera.
`flushes_seen` é o contador de flushes da sonda de render (instale-a antes);
ele ordena cada `draw` em relação ao flush do frame. `summary.flags` segue a
convenção do render: zero = completa; 1/2 armada/em andamento; 16 aninhado;
32 perda por disputa. Comandos 29–34, sem mudar a ABI v3/664 bytes.

No Windows, a sonda passou em três capturas reais (parado, após dirigir e
após recarga), sem flags nem chamadas estrangeiras. O único caminhão
observado foi o do jogador. Seu `draw` alterna o estado 6 (pilha world na
profundidade 0, desenha no passe do mundo) e o estado 12 (profundidade 1, não
desenha). A recarga criou outra entidade. O slot original foi restaurado,
conferido por leitura externa, e os hashes de originais/saves se mantiveram.

### Observação da transição de carregamento (`transition-*`)

Esta sonda é passiva: observa os virtuais `actors.car::init(phase)` e
`done(phase)`, sem chamar inicializações ou liberações adicionais. Não é a
correção automática do reload. Exige o pump que fixa a DLL e recusa instalação
com visual alocado; `visual-show` também recusa enquanto a sonda estiver instalada.
Compile e execute a bancada antes de abrir o jogo. Em uma partida de teste,
sem servidor e sem `session-start`, execute na ordem (prefixe cada linha
`engine ...` com `mp_launcher.exe --pid PID --dll CAMINHO\client_dll.dll`):

```text
mp_launcher.exe --pid PID --dll CAMINHO\client_dll.dll session-backup --backup target\analysis\transicao-NOVA
engine attach
engine pump-attach
engine render-attach
engine draw-attach
engine transition-attach
engine transition-blocks
engine transition-commands
engine transition-arm
# Com a captura armada: ir ao menu, carregar save de teste existente e parar.
engine transition-status
engine transition-report --output target\analysis\transicao-NOVA\captura-01.json
mp_launcher.exe --pid PID --dll CAMINHO\client_dll.dll session-stop --verify target\analysis\transicao-NOVA
```

`session-backup` só copia os saves e registra hashes; não carrega a DLL nem
cria um visual. `transition-report` lê um snapshot mesmo com captura ativa:
até 512 registros append-only, entrada/retorno, argumento da fase, thread,
callsite, entidade, cena/física/primeira entrada de modelo/quantidade de modelos,
flushes vistos e primeiro draw após cada grupo de fases. Os valores são leituras
de memória, não prova de ownership ou de que todas as filas foram consumidas.
`argument` preserva os bits de entrada e `phase` os interpreta como `i32`
(inclusive fases negativas). O nome `done` do diagnóstico corresponde ao
virtual `destroy(int)` do motor; é separado do destrutor do alocador.
`flags & 1` indica captura ativa, `flags & 4` truncamento e `flags & 32` perda
por contenção; uma captura incompleta não autoriza liberação. Rearmar troca a
geração, e leituras de uma geração antiga são recusadas. Evite consultar
repetidamente durante a transição. `transition-remove` restaura só os slots
que ainda pertencem à sonda; `pump-remove`, `session-stop` e `stop` também
removem a sonda. Feche o jogo normalmente após limpar, antes de reconstruir
a DLL. A primeira captura real passou: quatro fases de init da nova entidade
(`-10,-1,0,10`) precederam três fases de destroy da antiga (`-10,0,10`), na
mesma thread, sem perdas. A física antiga foi zerada durante destroy(0).
Isso comprova a observação, mas a nova cena já existia antes desse callback;
a automação ainda depende de localizar a transição global mais cedo.

`transition-blocks` amplia a mesma captura com os imports de `init_eblock`
e `destroy_eblock` do jogo. Instale depois de `transition-attach` e antes de
armar. Registra processador (`this`), item (`block`), dados/contexto e contadores
diretos do item, dois words crus da fila, retorno booleano e último caminhão
observado (`observed_car`). Nas linhas de bloco, cena/física/modelos referem-se
a esse último caminhão, que pode já estar obsoleto; as leituras não fixam sua
memória. `destroy_request_*` indica SOLICITAÇÃO, não destruição concluída.
O init pode executar fases aninhadas e retornar false com rollback do bloco
novo; isso não significa que o caminhão antigo foi destruído. Chamadas por
outros imports ou diretamente dentro de p3shared não passam por esses dois
hooks; o trace não comprova cobertura global completa.

As linhas também expõem até 16 entradas cruas de modelo (`models`), inclusive
as nulas, e `models_truncated` quando a quantidade declarada supera esse limite.
Isso permite observar outras entradas quando a primeira está vazia. As duas
assinaturas de bloco são thiscall com argumento ponteiro, bool em AL e ret4;
nunca são chamadas adicionalmente pela sonda. A parada remove os quatro hooks
de transição e confere sete slots externos no perfil do jogo. A extensão de
blocos passou na primeira recarga real: snapshot de 70 eventos, sem perdas,
todos os retornos true e fases aninhadas até profundidade 3. As solicitações
capturadas pertenciam a blocos da cena nova, depois da troca; não identificaram
o início global do reload. Os modelos/física antigos foram zerados em destroy(0).

`transition-commands` completa a captura com o binding `cmd_argv` do jogo.
Instale após `transition-blocks`, antes de armar. A sonda encaminha o original
uma vez e observa somente o retorno ao handler perfilado (`game+0xd3596`), na
thread proprietária. Copia apenas `argv[0]`, limitado a 31 bytes mais terminador;
não lê argumentos, nomes de saves ou caminhos de carregamento. Chamadas de
outros pontos continuam encaminhadas; chamadas estrangeiras desse handler são
contadas, sem serem incluídas na timeline. Retorno, pilha e LastError são
preservados. O comando é passivo e continua incompatível com visual alocado.

O evento `command_dispatch` ocorre após consultar argv e antes de executar o
ramo do comando. Expõe `command`, `command_status` e `handler_scene`, a leitura
de `this+0x14c` naquele instante. Cena/física/modelos do evento continuam sendo
do último caminhão observado, possivelmente obsoleto. `command_status` distingue
nome completo, ilegível, sem terminador e UTF-8 inválido; falhas de leitura não
deixam um prefixo parcial como nome válido. O status inclui `command_calls` e
`commands_installed`. Essas contagens são cumulativas desde a instalação, não
apenas da geração atual. A remoção restaura cinco hooks de transição e
`session-stop` verifica oito slots externos. Instalar a extensão é opcional;
não muda o layout da ABI v3/664 bytes.

A extensão de comandos foi validada em fixture própria sob Proton, incluindo
filtro de caller/thread, ponteiros inválidos, limites, ordem anterior ao init,
conflito de ownership e remoção automática. A primeira captura no jogo também
passou no Fedora/Proton: `game_menu` e depois `map`, com a cena antiga ainda
legível na entrada de `map`, antes do init novo e do done antigo. Foram salvos
48 registros iniciais e 61 após a limpeza, sem perdas/foreign/truncamento;
o relatório final foi analisado novamente depois da saída. O nome `load`
continua sendo apenas a simulação da fixture. Uma observação de `map` não
comprova todos os caminhos de carga nem a segurança de liberar o visual nesse
callback; não repetir reload com visual retido. Ver o registro no roadmap.

`transition-map` (comandos 52/53, mesma ABI v3/664) acrescenta uma sonda
passiva no callback do descritor perfilado de `map` (`game+0x1573c4`, slot
`+4` → `game+0xd2e90`). Instale depois de `transition-commands` e antes de
`transition-arm`; é recusada fora da thread proprietária, com captura armada,
callback em andamento ou instalação repetida. O preflight confere nome,
hash e callback do descritor duas vezes, alinhamento e ausência de aliasing
dos três words lidos (`argc`/`argv` do núcleo e handler atual em
`game+0x160df0`); nunca executa getters. O thunk original é cdecl sem
argumentos: lê o handler global e salta para o membro, ou retorna se nulo.
A sonda encaminha exatamente uma chamada, preservando pilha, EAX e LastError.

Eventos `map_enter`/`map_return` registram descritor, handler de entrada,
`argc`, ponteiro `argv` e handler corrente (antes/depois) e EAX do retorno.
Com `argc` em 1..32 copia somente `argv[0]`, como em `transition-commands`;
`argv[1..]` (nome do save) nunca é lido. EAX é diagnóstico: o membro não tem
retorno comprovado e o ramo de handler nulo devolve EAX indefinido. Chamadas
de outras threads são encaminhadas e contadas como foreign; remoção e rearmar
aguardam o retorno de qualquer chamada em andamento. O status expõe
`map_calls`/`map_installed`, a remoção restaura seis hooks e `session-stop`
verifica nove slots. A análise offline produz `map_calls` com despacho do
handler, primeiro init de outro ator, primeiro done do antigo, primeiro draw
após retorno e flushes no intervalo, sem inferir causalidade; argc/argv
alterados durante a chamada tornam a captura estruturalmente incompleta.
Para a sessão real, inserir `engine transition-map` logo após
`engine transition-commands` no procedimento acima. `session-backup` exige
uma pasta NOVA: não crie o destino antes. A primeira captura real passou
(menu → carregar save, 68 eventos íntegros): blocos e init do caminhão novo
ocorreram DENTRO de `map`, sem nenhum flush, e o done do antigo logo após o
retorno. Portanto não se pode liberar o visual dentro do callback; a
automação depende de adiar o comando. Ver o registro no roadmap.

### Preservar e analisar uma captura de transição

`engine transition-report --output ARQUIVO.json` salva o snapshot com os
registros crus e a análise, sem substituir um arquivo existente. A pasta pai
deve existir. Sem `--output`, continua imprimindo JSON no terminal. Os registros
são lidos integralmente na geração consultada antes de criar o arquivo; uma
falha de leitura do processo não deixa um snapshot parcial. Erros de escrita
são reportados e podem deixar um arquivo novo incompleto, que deve ser
descartado. Salve antes de encerrar o jogo: status e contagens não permitem
reconstituir registros perdidos depois da saída.

O binário Rust `transition_report` analisa esse JSON sem PID, injeção, DLL ou
assets do jogo. Depois do build release nativo, no Fedora:

```text
target/release/transition_report target/analysis/transicao-NOVA/captura-01.json --output target/analysis/transicao-NOVA/analise-01.json
```

No Windows, use `target\i686-pc-windows-gnu\release\transition_report.exe`
com os caminhos locais. O executável também pode analisar snapshots antigos
sem eventos de comando. O arquivo de análise também deve ser novo; omita
`--output` para imprimir no terminal.

A análise verifica limite de 8 MiB/512 registros, geração, contagens, thread,
profundidade e pares de entrada/retorno. Expõe fases assinadas por ator/cena,
retornos false de init de bloco, leituras de física/modelos após done(0) e
janelas entre comandos consecutivos, com o primeiro init/draw de ator
diferente e done do ator observado anteriormente. Índices começam em zero.
Essas janelas mostram ordem temporal, não causalidade nem duração do handler.
Endereços identificam observações locais e podem ser reutilizados pelo motor.
O contador `foreign_calls` é cumulativo desde a instalação; por precaução, um
valor não nulo sinaliza ressalva mesmo sem evento estrangeiro no snapshot.

`structurally_complete` descreve somente os registros presentes e seus pares,
não cobertura de todos os caminhos do motor nem completude de uma recarga.
Leituras ausentes ficam desconhecidas; modelos acima de 16 não comprovam
limpeza completa. Um init false não comprova rollback integral; diferença de
flushes não comprova consumo de todas as referências. `automation_validated`
permanece sempre false. O relatório ao vivo conserva os registros crus e
expõe `analysis_error` se não puder analisá-los. A ferramenta não libera
recursos e não altera o procedimento `session-prepare-load`/`visual-show`.

### Primeiro caminhão de teste (`visual-*`)

Com pump, render e draw instalados (nessa ordem), em partida carregada:

```text
engine visual-show --offset 400 0 0
engine visual-status
engine visual-hide
engine visual-release
```

`visual-show` cria (uma vez) uma instância privada do `truck.psm` de IA na fila
do loop e passa a desenhá-la logo após o desenho externo do caminhão do
jogador, deslocada em eixos do mundo (centímetros) e com a mesma rotação.
Interior, profundidade inesperada ou path/viewport nulos contam em `skipped`.
Sem física, colisão, HUD ou inserção no mundo. `visual-release` só libera
depois de ocultar, de ≥8 flushes e com o contador de acesso do item em zero;
se recusar, repita após alguns frames. `stop`/`pump-remove`/`render-remove`
recusam enquanto a instância existir, e `draw-remove` enquanto estiver visível.

### Conferir a liberação antes da carga (`visual-check`)

Com o pump instalado, `engine visual-check` coleta um relatório somente de
leitura na fila do loop. Não oculta, libera ou recria a instância. A janela
deve continuar processando o loop. A saída inclui thread atual/proprietária,
profundidades do pump/janela/draw/flush, contadores strong/residency/access,
presença dos bindings e idades dos flushes de ocultação e último desenho.
Exige o launcher e a DLL atualizados juntos; comando 51, ABI v3/664 preservada.

`ready=true` descreve somente aquele instante no loop; `visual-release`
repete as mesmas verificações imediatamente antes de liberar. Uma instância
visível retorna `refusal=visible`. Depois de ocultar, `draws_pending` e
`referenced` têm `retryable=true`; as demais recusas interrompem o preparo.
Contadores ilegíveis permanecem desconhecidos, sem serem substituídos por
zero. Não se libera dentro de draw/render/janela aninhados ou fora da thread
e fase do loop, mesmo que os contadores pareçam livres.

Os dois intervalos de flush são verificados separadamente, inclusive na volta
do contador u32. Ordem ambígua (diferença superior a i32::MAX) recusa liberação.
Um marcador separado mantém a guarda mesmo se a contagem de desenhos voltar
a zero. Instâncias nunca desenhadas dispensam a espera, mas continuam exigindo
os contadores `[1,1,0]` e bindings de liberação presentes.

`session-prepare-load` usa essa checagem após ocultar, aguarda somente as
recusas repetíveis e registra `preflight_checks` e `last_release_check`
(snapshot ANTES da liberação). Erros de transporte/ABI ou outras recusas
encerram a tentativa; não carregue o save sem `prepared=true`. Uma falha de
controle pode deixar o resultado da operação indeterminado: consulte o estado
antes de tentar novamente. Essa política também serve à parada da sessão.

`automatic_load_supported` permanece false. O `map` observado no jogo não
comprova a fase de liberação nem uma ABI para adiar/cancelar a carga. Este
bloco não altera esse handler e mantém `session-prepare-load` antes da carga
e `visual-show` depois. Não testar reload com visual retido.

O preflight passou também numa partida real no Fedora/Proton: recusou o visual
visível, indicou prontidão após ocultação e confirmou ausência após liberar.
O preparo concluiu com uma consulta/uma tentativa, oito slots restaurados e
seis hashes iguais após a saída. Não houve teste de liberação dentro de `map`
nem recarga com o visual retido; detalhes e limites no roadmap.

### Desenvolvimento em blocos e laboratório sem jogo

As próximas validações manuais serão agrupadas em marcos maiores. Análise dos
binários, testes de protocolo, fixtures x86, cenários de falha/recuperação e
builds devem avançar sem abrir o jogo a cada incremento. Uma sessão manual
deve reunir as verificações pendentes, preservando backup e limpeza. Resultados
de laboratório continuam separados das provas no jogo e na LAN.

O novo laboratório Rust exercita seis cenários completos por iteração:

```text
cargo run --release -p client_dll --bin load_lab -- --iterations 10000
```

São 60.000 cenários determinísticos, sem assets, PID, Steam ou servidor:
sucesso após drenar referências, recuperação de timeout, cancelamento antes
da execução, resultado desconhecido sem replay, troca inesperada de mundo e
confirmação atrasada. O relatório conta operações simuladas e mantém
`game_tested=false`/`automatic_load_supported=false`. No Windows também há
`target\i686-pc-windows-gnu\release\load_lab.exe` após `dev_env windows`.

O controlador `deferred_load` mantém uma carga por vez, argumentos próprios
(até 32/4096 bytes, inclusive bytes não UTF-8), prazo monotônico, tokens de
operação e estados explícitos de recuperação. Nunca guarda ponteiros argv
da pilha original nem reconstrói comandos juntando strings. Somente o loop
pode emitir ações; liberar precede executar, e retorno do callback ainda
exige uma nova geração de mundo pronta antes de recriar. Resultado desconhecido
fica bloqueado até recuperação explícita; timeout/cancelamento não executa
trabalho atrasado. Logs/JSON do controlador omitem argumentos e nomes de saves.

O adaptador de bancada agora instala um hook num descritor **próprio** com o
formato x86 de 16 bytes e callback em `+4`. Faz troca/restauração condicional,
mantém argc/argv próprios durante a chamada e executa o callback original da
fixture uma vez, sem passar pelo hook novamente. Nenhum endereço do jogo é
aceito por esse adaptador. Para exercitar oito cenários por iteração:

```text
cargo run --release -p client_dll --bin load_lab -- --adapter --iterations 10000
```

Esse modo usa callbacks cdecl reais do próprio laboratório; hide/check/release,
mundos e gerações continuam simulados. Inclui chamada aninhada recusada,
alteração externa do descritor/argv, mudança e retorno dos mesmos words (ABA),
cancelamento e prazo vencido entre emissão e execução da ação. Confere os
argumentos depois de destruir sua origem, restauração, retorno e LastError;
a bancada Windows x86 também confere a pilha.

Se argv mudar durante a chamada, o adaptador preserva os words externos e
retém seus argumentos, bloqueando outro binding até recuperação explícita.
Essa recuperação exige prova de ausência de leitores; somente mudar argv
não basta. Descartar o host sem essa prova deixa a memória retida, em vez de
liberá-la sob ponteiros possivelmente publicados. Não há repetição automática
de callback nem recriação após resultado incerto. Os relatórios omitem os
argumentos e mantêm `game_tested=false`/`automatic_load_supported=false`.

O host próprio serializa suas escritas e versiona argc/argv. Isso não comprova
quiescência dos globais reais, nem detecta ABA no descritor do jogo. Ainda é
necessário comprovar suspensão/replay/contexto do callback original, obter
gerações reais de lifecycle e conservar a configuração do visual antes de
conectar a automação ao jogo. O preparo manual continua obrigatório antes de
carregar com o mod visual ativo.

Para a próxima sessão agrupada, a consulta externa `memory_tool commands --pid
PID` audita o registro de comandos e o descritor de `map`, com perfil/hash,
limite de entradas, detecção de ciclos e releitura contra mudanças. Não injeta,
não lê argv/argumentos e não executa comandos. Do buffer associado a `cmd_add`,
lê apenas ocupação e contador; não afirma que seja uma fila de execução.
A correspondência do callback é diagnóstico, não autorização para chamá-lo.
Esta consulta nova ainda não foi validada numa partida real.

### Teste no jogo com sessão (um comando)

Compile e verifique tudo antes: `.\target\debug\dev_env.exe verify`. Abra o
jogo, carregue o save e deixe a janela em primeiro plano. Em outro terminal:

```text
multiplayer_server --bind 0.0.0.0:39000
mp_launcher.exe --pid PID --dll CAMINHO\client_dll.dll session-start --server IP:39000 --backup target\analysis\sessao-NOVA
mp_launcher.exe --pid PID --dll CAMINHO\client_dll.dll session-status
# Carregar um save pelo menu: o gate libera, carrega e recria (ver abaixo).
mp_launcher.exe --pid PID --dll CAMINHO\client_dll.dll session-status
mp_launcher.exe --pid PID --dll CAMINHO\client_dll.dll session-stop --verify target\analysis\sessao-NOVA
```

`session-start` instala as sondas, conecta no modo `drawn` e mostra todos os
remotos (`--player all`), com o skin do caminhão de IA (`--no-skin` para o
fallback). A pasta de `--backup` precisa ser nova; ela guarda cópias dos saves
e o SHA-256 dos cinco originais e dos saves. `session-status` lista os IDs dos
remotos. `session-stop` só termina depois de liberar o visual com segurança
(mantenha a janela em foco), remove as sondas e confere os slots e os arquivos.
Depois disso, feche o jogo normalmente: a DLL fica fixada até ele encerrar.
`session-prepare-load` oculta e libera a instância com as mesmas guardas da
parada, mantendo rede e sondas. Aguarde `prepared=true` antes de abrir/carregar
o save. A primeira tentativa de reload com a instância retida terminou em
violação de acesso; detectar a nova entidade depois da recarga não protege os
recursos durante a destruição do mundo antigo. Com `--no-load-gate`, esse
preparo manual continua obrigatório antes de carregar. Com o gate (padrão),
a carga pelo menu é adiada automaticamente; ver a seção seguinte. O gate
foi validado no jogo com eco (duas cargas).
Para testar num só PC, inicie o servidor com `--echo`: ele também devolve a
cada cliente a própria pose como o jogador `1000000000 + id`. Com
`session-start --server 127.0.0.1:39000 --offset 400 0 0`, o seu caminhão
aparece espelhado 4 m ao lado. O caminho passa por rede, histórico,
interpolação (100 ms atrás) e desenho. Sem `--echo`, use dois PCs na LAN, cada
um com sua sessão apontando para o mesmo servidor.

### Carregamento e saída com o visual ativo (gate de comandos)

`session-start` agora instala também o **gate ativo de `map`** (comandos
54–57, mesma ABI v3/664), depois do draw e antes de `visual-show`. Use
`--no-load-gate` para voltar ao procedimento manual e `--load-timeout SEG`
(padrão 120, 1–600) para o prazo entre o pedido de carga e a recriação.
Com o gate instalado não é preciso rodar `session-prepare-load`. Basta
carregar um save pelo menu do jogo:

1. o comando `map <save>` chega ao descritor perfilado (`game+0x1573c4`+4);
   com o visual alocado, o hook copia argc/argv (no máximo 32 argumentos,
   4096 bytes), oculta o visual e **não executa** a carga naquele instante;
2. no pump (`PeekMessageA`, profundidade 1, fora de `cmd_execute`), o gate
   espera a mesma política de liberação (≥8 flushes e item `[1,1,0]`) e
   libera a instância;
3. publica argc e um argv próprios em `prism3d+0x778f0/+0x778f4` e chama
   o callback original uma vez, exatamente como `cmd_execute`
   (`prism3d+0xe010..0xe01c`). Depois, restaura os words se ainda forem
   seus. O argv reexecutado fica alocado até o fim do processo;
4. quando o draw observa um ator novo, recria o visual com os mesmos
   parâmetros de `visual-show`, se ele estava visível no pedido.

Sem visual alocado, a chamada é encaminhada sem mudança. Um segundo pedido
enquanto há outro em andamento é descartado (`swallowed_busy`). Uma chamada
aninhada durante a reexecução é encaminhada (`replay_nested`). Com argumentos
ilegíveis ou sem testemunha de mundo, o pedido é descartado e o visual é
liberado sem carregar (`dropped`/`fallback_releases`). Nesse caso, carregue de
novo. Se o prazo vencer antes da execução, nada é carregado nem liberado
(`phase=recoverable`, `reason=deadline_expired`). O próximo pedido assume a
restauração pendente. Para ver o estado, use `session-status` (campo
`load_gate`) ou `engine load-gate-status`. Os nomes dos saves nunca são
registrados.

**Outros comandos que apagam a cena.** O mesmo handler (`game+0xd3570`, thunk
`+0xd2e90`) apaga a cena (`this+0x14c` via `0xcc4a0` e destrutor) em `map`,
`preview`, `edit`, `exit`, `menu` e `quit`. `game_menu` e `pause` não apagam
a cena. O gate instala o mesmo shim nos seis descritores. Na partida, todos
precisam estar registrados com o callback perfilado e o hash correto, ou a
instalação é recusada. O comando é identificado pelo `argv[0]`, que o
`cmd_execute` publica antes da chamada (até 15 bytes). `map` e `preview` com
argc<2 só retornam no handler e são encaminhados. Depois de
`exit`/`menu`/`edit`/`preview` não há mundo de jogador: o job termina no
retorno (`restore_pending=true`) e o visual é recriado quando o draw vê o
próximo ator novo, por exemplo ao carregar um save a partir do menu principal.
Depois de `quit` nada é restaurado. `exit` chama `menu` por dentro; essa
chamada aninhada é encaminhada. O status traz `last_command`. Fechar a janela
(Alt+F4/X) pode não passar por `quit` e continua sem cobertura.

`session-stop` e `session-prepare-load` aguardam o gate assentar antes de
liberar; `session-stop` remove o gate antes das sondas. `engine
load-gate-attach [--timeout SEG]` e `engine load-gate-remove` existem para
uso avulso: instale depois de pump/render/draw. A remoção é recusada enquanto
um carregamento adiado estiver ativo. A sonda passiva `transition-map` e o
gate são mutuamente exclusivos no mesmo descritor.

**Validado no jogo** (Fedora/Proton, eco num PC): duas cargas de `test_mp` pelo
menu com o caminhão espelhado visível, que voltou nas duas, sem crash, com nove
slots e sete hashes conferidos. Antes disso, foi validado na bancada própria
(fixture com callback cdecl, argv destruído após o retorno, troca de ator e
contagem de instâncias PSM vivas no momento da carga). LAN e sessões longas
pendentes; mantenha a janela em primeiro plano. A cobertura de
`preview`/`edit`/`exit`/`menu`/`quit` foi validada só na bancada (descritor
`exit` próprio, menu sem jogador, restauração após `map`), sem teste no jogo.
Novo jogo, morte/reinício e fechar a janela não são interceptados. Continue
rodando `session-stop` antes de fechar o jogo. `automatic_load_supported` continua `false` até ampliar a matriz.

### Comportamento do caminhão remoto (referência)

- **Suavização:** cada jogador remoto guarda 16 amostras (tempo de recepção
  local). O desenho fica 100 ms atrás da mais recente, com interpolação linear
  de posição e de quaternion pelo arco curto (nlerp). Se faltar amostra, prevê
  no máximo 200 ms à frente. Acima de 20.000 cm/s implícitos, salta direto ao
  destino (teleporte). Sem amostra há mais de 1 s, não desenha.
- **Vários remotos:** `--player all` desenha cada remoto fresco com a mesma
  instância (até 16 por frame). `engine remotes` lista ID, sequência, idade,
  amostras, posição e rotação.
- **Skin:** `visual-show --shader models/ai/truck/shaders/truck.shd` aplica o
  shader do `car.def` de IA a todas as partes via `psm_set_shader`.
  `session-start` faz isso por padrão (`--no-skin` mantém o fallback); o status
  informa `skinned_parts`.
  O primeiro teste real ficou preto mesmo com três partes configuradas;
  `skinned_parts` não comprova textura correta. O código agora fornece a
  iluminação da cena durante o draw e remove o callback/contexto da instância
  logo depois. O usuário confirmou a textura correta após movimento e após
  carregar/recriar com liberação prévia; não houve travamento relatado no reteste.
- **Recarga:** a recriação ao detectar uma entidade nova passou somente na
  bancada; o primeiro reload real com a instância retida fechou o processo.
  `session-prepare-load` antes de carregar e `visual-show` depois passaram no
  reteste real, com limpeza e hashes conferidos. Manter esse procedimento.
  `recreated`/`recreate_pending` são diagnósticos da tentativa automática;
  `fault=2` indica interrupção, não cobertura de todos os riscos do carregamento.
- **Métricas:** `draw_cost_avg_ns`/`draw_cost_max_ns` (custo das nossas chamadas
  ao motor por frame) e `frame_interval_avg_us` (tempo médio de frame local) em
  `visual-status`/`session-status`.
- **Arquivos:** `session-verify --backup DIR` compara originais e saves com o
  backup sem abrir o processo nem chamar a DLL, inclusive após saída/crash.
  As cópias dos saves ficam na pasta do backup. Os argumentos globais `--pid`
  e `--dll` continuam aceitos/exigidos pela CLI, mas não são usados nessa ação.

### Caminhão remoto pela rede (`--mode drawn` e `--player`)

O protocolo agora aceita um campo opcional `rotation` (quaternion x, y, z, w;
unitário, com tolerância de 2%) em `ClientUpdate`/`ServerUpdate`. Pacotes sem
rotação continuam válidos, e o servidor retransmite o campo sem alterá-lo.

Com pump e draw instalados, `connect --mode drawn` envia a pose que a sonda de
`draw` observa no caminhão local: posição e rotação no mesmo sistema de
coordenadas usado para desenhar, em centímetros. Sem um `draw` recente (menos
de 0,5 s; por exemplo, com o jogo fora de foco), o envio pausa e informa o
motivo em vez de mandar uma pose velha.

`engine visual-show --player ID --offset 0 0 0` desenha o caminhão de teste na
pose mais recente do jogador remoto `ID`, somada ao offset; sem rotação
recebida, usa a orientação identidade. Amostras com mais de 1 s não são
desenhadas (contam em `skipped`). O histórico já aplica interpolação e
extrapolação limitada, conforme a referência acima; validação no jogo pendente.
`--player 0` mantém o comportamento anterior (jogador local mais o offset).
`visual-status` informa o alvo em `target`.

Para anotar uma desmontagem com os nomes de bindings recuperados:

```text
engine_report aa/lib/game.dll disassemble --rva 0x5a6a0 --length 300 --resolver 0xcdce0 --resolver 0xce970
```

`--resolver` (repetível) recebe o início de uma rotina de `GetProcAddress` e
anota cada operando absoluto que coincide com um slot recuperado. O resolver
do núcleo em `game.dll+0xcdce0` tem 151 bindings `gfx_*`; o de PSM, 87.

## Retomada no Windows com Rust 1.99.0

O toolchain fixado também se aplica ao Windows GNU x64. O `dev_env windows`
agora detecta o host: no Windows usa as ferramentas distribuídas pelo Rust
ativo; no Fedora preserva o fluxo de MinGW/rustup local. A configuração dos
linkers é passada somente ao Cargo filho, sem sobrescrever `.cargo/config.toml`.
O driver LLVM é copiado para cache por versão dentro de `target/toolchain/`;
um cache com conteúdo diferente é recusado em vez de sobrescrito.

No host GNU Windows, instale os componentes e compile o helper. O preparo de
PATH/linker abaixo vale só para este terminal e também atende configurações
locais antigas que apontavam para Rust 1.98.1:

```powershell
rustup component add llvm-tools-preview
rustup component add rust-mingw --target x86_64-pc-windows-gnu
rustup component add rust-mingw --target i686-pc-windows-gnu
$aaRustRoot = rustc --print sysroot
$aaGnuBin = Join-Path $aaRustRoot 'lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained'
$env:PATH = $aaGnuBin + ';' + $env:PATH
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $aaGnuBin 'x86_64-w64-mingw32-gcc.exe'
cargo build -p asset_tools --bin dev_env --locked
.\target\debug\dev_env.exe windows
.\target\debug\dev_env.exe windows --tests
.\target\i686-pc-windows-gnu\release\test_host.exe target/windows-tests.json
.\target\debug\dev_env.exe windows --clippy
```

`--tests` gera os artefatos e o manifesto; `test_host` efetivamente os executa.
Para a verificação completa num comando (build release x86 e nativo, bancada x86,
bancada nativa, Clippy x86/nativo e rustfmt, com a soma de testes passados):
`.\target\debug\dev_env.exe verify`. Ele substitui a sequência manual acima no
Windows; no Linux continue com `proton_host`/`test_host`.
O build release deve vir antes da bancada, que carrega a DLL release real.
O build nativo também atualiza os executáveis de servidor e assets em
`target/x86_64-pc-windows-gnu/release/`; compilar testes não atualiza esses
programas. No Windows, use esse servidor para `--echo` e o launcher/DLL de
`target/i686-pc-windows-gnu/release/` para a partida. Não reutilize executáveis
antigos de outro diretório apenas por terem o mesmo nome.
`windows --native --tests` cobre o workspace x64 e grava
`target/windows-native-tests.json`; execute-o com
`target/x86_64-pc-windows-gnu/debug/test_host.exe`. O mesmo helper aceita
`windows --native --clippy` e `windows --check`.

O GCC precisa localizar `ld.exe`; o helper passa o diretório correspondente
com `-B`, prepara o PATH e seleciona o linker host também para build scripts e
macros. Em x86 usa GCC/GNU ld do componente `rust-mingw` ativo. Na retomada,
`rust-lld` gerava executáveis que encerravam ao recuperar um panic; o mesmo
teste passou com GNU ld. Por isso, não reutilize o antigo linker x86 deste
ambiente. Não misture um executável `dev_env` antigo com estes comandos sem
recompilá-lo.

No Windows, use `mp_launcher.exe --pid PID_ATUAL --dll CAMINHO_DA_DLL ...`
diretamente, sem `proton_host`. O ciclo `connect`, consultas, `stop` e `unload`
continua fora de `DllMain`. A regra de residência de `engine pump-attach`
também vale no Windows: após usá-la, encerre o jogo antes de reconstruir a DLL.
Mantenha a partida em primeiro plano durante as consultas `engine`: neste host,
as mensagens expiraram fora de foco e passaram ao ativar a janela. Em timeout,
consulte o estado, restaure o foco e remova o dispatcher antes de descarregar;
se a remoção falhar, mantenha o módulo e encerre/reabra o jogo normalmente.

## Ambiente e compilação Windows (registro histórico)

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

### Linker histórico e migração

No primeiro ambiente 1.98.1, a falta do script GNU x86 levou ao uso de
`rust-lld`, com `llvm-ar` copiado como `llvm-dlltool`. Essa configuração era
local e ignorada pelo Git. Foi substituída pelo procedimento 1.99.0 acima,
que usa GNU ld também em x86 e não grava caminhos particulares no repositório.
Após trocar toolchain ou limpar `target/`, recompile o helper e repita o fluxo
atual. Não copie dependências por cima das DLLs originais.

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
