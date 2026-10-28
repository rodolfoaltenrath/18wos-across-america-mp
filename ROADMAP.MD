# Roadmap — Across America multiplayer em Rust

Última atualização: 27/10/2026. Este arquivo descreve o projeto, não uma promessa
de prazo. Checkboxes concluídos exigem implementação ou evidência registrada.
O objetivo é desenvolver ferramentas de assets e multiplayer para a instalação
original Steam de **18 Wheels of Steel: Across America**, com todo o código e
automação próprios em **Rust**. O jogo original permanece instalado separadamente
e não será distribuído pelo projeto.

**Ambiente de continuidade informado pelo proprietário: Linux.** Ferramentas e
servidor nativos e DLL para Windows x86 têm fluxos de compilação diferentes.
A seção 8 contém o procedimento Linux. A análise de continuidade abaixo registra
build e testes nativos em Fedora. A entrega de integração abaixo acrescenta
build cruzado, bancada Win32 sob Proton e carga/controle da DLL no jogo original.
Ainda não houve teste de coop entre computadores distintos.

## 1. Forma de trabalho e autonomia

**O proprietário autoriza grandes alterações no projeto.** Refatorações amplas,
reorganização de diretórios/crates, ferramentas novas em Rust, evolução do
protocolo e substituição de implementações provisórias estão autorizadas quando
ajudarem a cumprir o objetivo. Registrar a razão e manter uma migração clara.
Não é necessário interromper o trabalho para aprovar cada decisão reversível.

Executar blocos completos de investigação, implementação, compilação, validação
e documentação. Uma etapa concluída deve conduzir à seguinte quando houver
trabalho independente já autorizado. Não parar apenas porque foi encontrada
uma dificuldade técnica ou para oferecer executar o que já foi solicitado.
Atualizações curtas de progresso servem para informar; não são pedidos de
aprovação. Esta orientação não exige expor raciocínio interno nem contorna
limites de execução da ferramenta.

Perguntar quando houver dependência real: ação manual no jogo, informação que
não possa ser obtida localmente, escolha de produto que mude muito o escopo ou
ação destrutiva fora do que já foi autorizado. Agrupar ações manuais, explicar
o resultado esperado e continuar o restante do trabalho enquanto aguarda.
Não solicitar repetidamente autorização já concedida.

Preservar trabalho do usuário, originais e saves. Preparar backup e rollback
antes de alterações locais de integração. Publicar um repositório ou uma release
depende de instrução para publicar e de um destino conhecido; este roadmap não
é uma ordem para enviar arquivos ao GitHub agora.

Essas orientações também estão em [AGENTS.md](AGENTS.md), para acompanhar o
repositório e orientar futuras sessões de desenvolvimento.

## 2. Estado real de partida

| Área | Implementado e validado | Ainda falta |
| --- | --- | --- |
| Workspace | Cargo com `asset_tools`, `multiplayer_server`, `client_dll` | Reproduzir o ambiente em outra máquina e automatizar o preparo |
| Assets | Varredura, listagem, extração ZIP/SCS, conversão de texturas, reempacotamento e inspeção binária/PE | Testar instalação de pacote no jogo, manifesto, restauração e distribuição só das alterações próprias |
| Servidor | Tokio UDP, retransmissão com alvo de 60 Hz, IDs, timeout, limites e validação básica | Sessão explícita, sincronização de relógio, rotação, controle de tráfego e mundo compartilhado |
| DLL | x86 no jogo sob Proton, perfil por hash, configuração ABI, worker, UDP, dispatcher e sonda do loop | Matriz completa de estados do jogo e APIs de entidades |
| Host de teste | `client_probe`, controlador entre processos e `test_host`; bancada x86 passou sob Proton | Testes de sessão longa, saída abrupta e duas máquinas |
| Investigação | `memory_tool` Rust: processos, regiões, busca, filtros, comparação, ponteiros, observação e envio externo | Perfis de versões e ciclo de vida do veículo |
| Coordenadas | Cadeia validada após movimento, reinício e comparação exata com save | Escala física, outros estados/versões e amostragem consistente |
| Multiplayer visual | Criação/destruição privada de `actors.car` validada; nenhum remoto no mundo | Inicialização, modelo, inserção, transform e remoção da entidade visual |
| GitHub | Remoto `rodolfoaltenrath/18wos-across-america-mp`, histórico inicial Apache-2.0 preservado, exclusão do jogo | Manter staging auditado e documentação atualizada em cada entrega |

O estado atual **não é multiplayer jogável**. Transmitir números e recebê-los
em outra DLL não cria automaticamente um caminhão no mundo do jogo.

### Evidência de memória que deve acompanhar o código

Arquivo local investigado: `aa/lib/game.dll`, x86.

```text
SHA-256:
ccb327c2a5c2f7d01d74d94629cf65847ee1535ca9020b3a5fc5d412a8ffbc7d

Cadeia base:
game.dll+0x160df0->0x14c->0x4610->0x8->0x8->0x1d0->0x3e4

X: acrescentar ->0x30     f32
Y: acrescentar ->0x34     f32
Z: acrescentar ->0x38     f32
Orientação candidata: quatro f32 consecutivos a partir de ->0x14
```

Cada `->` lê um ponteiro antes de somar o offset; offsets são hexadecimais.
A ordem, convenção e normalização dos componentes de orientação precisam de
validação antes de usá-los como quaternion do protocolo.

- [x] A cadeia acompanhou movimento e mudança de orientação na mesma sessão.
- [x] Após fechar/reabrir e carregar `mp_test2`, resolveu novamente os dados.
- [x] A base do módulo mudou de `0x04e10000` para `0x04d70000`; X passou de
  `0x13a1fc58` para `0x1307f818`, mantendo os mesmos offsets.
- [x] X/Y/Z retornaram `1107795.75`, `23592.041015625`, `798043.3125`, iguais aos
  três `f32` gravados no save; os quatro componentes de orientação coincidiram.
- [x] O envio externo foi recebido por uma DLL x86 no `client_probe`: 240 pacotes
  em quatro segundos antes do reinício e 180 em três segundos depois.
- [x] Última execução registrada: 14 testes do workspace em x64, sete do cliente
  em x86, além de Clippy e formatação.
- [ ] Validar escala em metros, eixos, origem do mundo e orientação em curvas,
  rampas, colisões e diferentes câmeras.
- [ ] Validar outra máquina e o ciclo menu → carregar → dirigir → sair → carregar.

`mp_test` e `mp_test2` foram criados pelo usuário e apenas lidos pelas ferramentas.
Nos dois saves locais, os dados comparados começaram em `0x1ec33`; isso não define
um formato universal de save. Capturas, dumps, disassembly e logs estão em
`target/` e não irão para o Git. A evidência resumida acima permite retomar sem
transportar esses arquivos. PIDs e endereços absolutos são históricos.

### Análise de continuidade no Linux — 27/10/2026

- Ambiente: Fedora 44, `rustc`/Cargo 1.99.0 da distribuição, host
  `x86_64-unknown-linux-gnu`. É uma validação adicional com compilador diferente
  do Windows registrado; não houve fixação/migração do toolchain.
- Passaram `cargo build --workspace --locked`, `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings` e
  `cargo test --workspace --all-targets --locked`: **nove testes**, sendo cinco
  de assets, dois do cliente portável e dois de protocolo/servidor, incluindo
  relay UDP entre sockets locais. Os testes Win32 e `memory_workflow` são
  excluídos por `cfg(windows)`; os 14 testes históricos não foram repetidos aqui.
- `target/debug/asset_tools scan ..` abriu os três SCS sem extrair ou modificar
  arquivos: `aa/base.scs` (7118 entradas), `aa/patch_1_10.scs` (403) e `data.scs`
  (76). `inspect-pe` confirmou `prism3d.exe` e `aa/lib/game.dll` como x86.
  O SHA-256 de `game.dll` coincide com a referência acima; a cadeia não foi
  lida num processo nesta análise, portanto não está validada sob Proton.
- Não foram encontrados `rustup`, bibliotecas Rust para `i686-pc-windows-gnu`
  ou `i686-w64-mingw32-gcc`. Build cruzado não executado. Wine está disponível,
  mas nenhum teste do jogo/host Win32 foi feito neste Linux.
- O clone já aponta para o remoto oficial. Não precisa ficar na raiz Steam;
  as ferramentas aceitam caminhos externos. Nenhum original/save foi alterado
  e nenhuma operação de commit/push foi feita.

A revisão confirma as prioridades M1/M2 → M4: a configuração atual depende de
variáveis de ambiente e não verifica o hash; uma falha em `Source::sample`
encerra o worker, sem distinguir carregamento temporário; X/Y/Z são resolvidos
e lidos separadamente. O protocolo transporta posição e sequência, identifica
clientes pelo endereço UDP e não negocia sessões. Reiniciar com o mesmo endereço
e sequência menor causa descarte enquanto o peer anterior existir. Essas
limitações pertencem a M1–M3. Nenhum caminho de criação/atualização/remoção de
entidades do motor está implementado; esse continua sendo o principal risco
para o MVP visual.

**Próximo bloco:** preparar o build cruzado e o perfil/configuração/ciclo
controlado da DLL em host Rust próprio; depois validar no jogo e investigar uma
entidade visual mínima. Os marcos M0–M4 permanecem incompletos.

### Entrega da integração x86 no Fedora/Proton — 27/10/2026

**Entregue:** perfil `steam-aa-1.10-ccb327c2`, configuração explícita ABI v1,
carregador/controlador x86 e captura real no processo do jogo. O desenvolvimento
passa a usar Rust 1.99.0, fixado em `rust-toolchain.toml`. O host nativo Fedora
e o toolchain rustup local têm o mesmo commit de compilador; o Windows 1.98.1
anterior permanece como evidência histórica, não como requisito do fluxo atual.

Arquivos principais: `api.rs`, `profile.rs`, `controller.rs`, `pe_exports.rs`,
`mp_launcher`, `proton_host`, `test_host`, `dev_env` e os dois testes entre
processos. Nenhum crate adicional foi necessário. SHA-256 usa `sha2` e suas
dependências registradas no lockfile.

- MinGW local em `target/toolchain/`, sem instalação administrativa: GCC
  16.2.1, binutils 2.46.0, CRT/headers 13.0.0. `dev_env` extrai RPMs confiáveis
  num destino novo e executa build/check/Clippy x86; não modifica `.cargo`.
- `client_control` recebe um bloco sem ponteiros, de 1536 bytes, com versão,
  tamanho, comando, configuração de 976 bytes, contadores e diagnóstico.
  O layout é testado nativamente e em x86. O host mantém a propriedade do bloco
  até o retorno; a DLL não devolve alocações Rust pela ABI.
- Carregamento via `LoadLibraryW`, controle via export próprio e descarga via
  `FreeLibrary`, sempre fora de `DllMain`. O controlador resolve exports PE32
  nas imagens remotas, incluindo forwarders. Aloca somente dados graváveis,
  sem shellcode, hooks ou alteração de executáveis. Um mutex nomeado por PID
  serializa os controladores próprios. Timeout retém o payload remoto e exige
  reabrir o alvo antes de novas operações, sem terminar threads à força.
- No modo jogo, hash/arquitetura são verificados antes de transmitir. Posição
  é lida em 12 bytes após resolver a cadeia e a resolução é repetida para detectar
  troca de estrutura. Isso não garante atomicidade da física nem valida orientação.
  Indisponibilidade temporária suspende envios e permite retomada; falha fatal
  de perfil é diagnosticada. A recepção permanece ativa durante indisponibilidade.
- Validação: 12 testes nativos passaram; nove testes unitários x86 e os dois
  testes entre processos passaram sob Proton Experimental
  `experimental-11.0-20261014-x86_64`. A bancada testa carga idempotente, ABI
  incompatível, início duplicado, captura real de fixture, recepção UDP,
  indisponibilidade/recuperação, parada, reinício, descarga e nova carga.
  Formatação e Clippy nativo/x86 passaram com avisos tratados como erros.
- No jogo original, a DLL transmitiu ao relay nativo Fedora e recebeu dados de
  `client_probe`. Um probe recebeu 3599 atualizações em cerca de 60 segundos.
  Houve parada/descarga e nova carga no mesmo processo, sem substituir arquivos.

Leituras reais, também recebidas pelo probe via relay:

| Cenário | X | Y | Z |
| --- | ---: | ---: | ---: |
| Primeira partida parada | 90120.0 | 18187.958984375 | 665086.125 |
| Após dirigir/fazer curva | 95039.0625 | 18208.150390625 | 662728.375 |
| Após menu e outra partida, com movimento | 1109895.125 | 23576.103515625 | 814360.4375 |

No ciclo menu → outra partida, o endereço de X mudou de `0x0c27c8f0` para
`0x0f0945a8` (históricos, nunca configuração). O worker permaneceu executando,
registrou 497 amostragens indisponíveis e manteve o contador de envios em 10453
durante o intervalo observado. Resolveu a nova cadeia e retomou automaticamente,
sem reiniciar a DLL. Trocas de veículo/reboque, todas as câmeras, menu mantendo
objetos legíveis, saída abrupta e sessão longa continuam pendentes.

Cuidados de ambiente comprovados nesta sessão:

- Wine 11 do sistema ficou na inicialização de um prefixo novo; a tentativa foi
  encerrada somente nesse prefixo. A execução validada usa Proton da Steam.
- Bancada com fixtures deve nascer com seu wineserver em um único runtime e
  prefixo exclusivo. Chamadas em namespaces distintos reutilizando o wineserver
  da partida falharam com acesso negado; `test_host` resolveu o cenário.
- Uma chamada inicial a `proton run` sem o ambiente completo da Steam removeu
  S: do prefixo. O link foi restaurado do backup, e `proton_host` passou a usar
  `runinprefix`; a validação bem-sucedida ocorreu depois da restauração.
- Foram preservadas cópias locais do prefixo e de `aa/` em `target/backups/`.
  Para restaurar, fechar jogo e wineserver desse prefixo, preservar o estado
  atual em outro destino e copiar de volta a árvore de backup correspondente;
  não mesclar um prefixo em uso. S: foi restaurado isoladamente da cópia.
  Os hashes de `aa.exe`, `prism3d.exe`, `game.dll` e dos três SCS foram conferidos
  iguais ao final. Ferramentas não gravaram saves. Não houve commit/push.

**M4 iniciado por inspeção estática:** `game.dll` exporta `DLL_game_init`,
`DLL_register`, `DLL_shutdown` e `DLL_uiedit_init`. `p3shared.dll` tem SHA-256
`7df43b55e6b34e3ac2017af9566665bd935895ff02a2a916b099bb34045d30a3` e exporta
símbolos C++ de `entity_create`, `entity_destroy`, `entity_get_struct`,
`entity_list_structs` e `entity_register`, além de métodos de `entity_t`.
Isso fornece pontos de investigação, sem comprovar que uma entidade genérica
possa representar um caminhão. Nenhum desses exports foi chamado.
Disassembly permanece local em `target/analysis/`.

**Próximo bloco:** seguir os registros de tipos e construtores do caminhão,
identificar o tick/thread do motor e ownership; só então experimentar uma
entidade visual sem colisão. M1 precisa ampliar a matriz de estados, M2 precisa
testar saída abrupta/rede prolongada e M3 ainda não define sessões/reconexão.
Não há caminhões remotos nem teste LAN entre computadores; M0/M1/M3/M4 não
estão concluídos. O ciclo controlado central de M2 está entregue.

### M4 — Descoberta reproduzível dos registros — 27/10/2026

`engine_report` nativo Rust analisa PE32 sem carregar código: imports/exports,
RVA → arquivo, disassembly x86 com `iced-x86`, referências e candidatos a
registros de entidades. Limites: arquivo de 64 MiB, 96 seções, tabelas/string
limitadas, recusa de RVAs fora dos bytes e de seções ambíguas. Fixtures próprias
cobrem os formatos e registros que possuem somente uma variante de construtor.

`memory_tool entities --pid PID_ATUAL` verifica os hashes de `game.dll` e
`p3shared.dll` e percorre a lista identificada em `p3shared.dll+0x49a7c`, sem
chamar APIs do motor. Há limites de 4096 nós, detecção de ciclo/alinhamento,
checagem de strings e rejeição de troca da cabeça durante a leitura.
Foram encontrados **80 registros** na imagem do jogo e os mesmos 80 tipos
na partida aberta sob Proton, incluindo `actors.car`, `actors.trailer` e
`actors.ai`. As alocações da variante selecionada pelo booleano `false` têm
respectivamente `0x2118`, `0x1244` e `0x1f80` bytes. A variante `true` desses
tipos é nula e não deve ser chamada. O significado de produto dessas variantes
ainda não foi identificado; os nomes do código refletem somente o booleano.

O factory exportado recebe registro, booleano e armazenamento opcional; registra
o destrutor no objeto e chama uma inicialização virtual. Os destrutores observados
distinguem armazenamento fornecido pelo chamador de alocação do motor. Essas são
conclusões da inspeção, ainda sem autorização técnica para executar os callbacks
numa thread arbitrária. Relatórios e disassembly permanecem em `target/analysis/`.
Validação deste bloco: 16 testes nativos; Clippy nativo/x86; bancada x86 sob
Proton com os testes de parsing e integração. Nenhum construtor foi chamado.

### M4 — Dispatcher para a thread da janela — 27/10/2026

Entregues `engine_bridge`, ABI separada `EngineControl` de 448 bytes e comandos
`mp_launcher engine attach/probe/registry/inspect/detach/status`. A configuração
e a ABI de rede v1 não mudaram. O dispatcher usa mensagem registrada e subclass
Win32; não altera instruções do motor. Seleciona uma janela do próprio processo
ou aceita um handle explícito. Mensagens comuns seguem para o procedimento
original com `CallWindowProcW`, incluindo janelas ANSI.

A fila limitada a 16 pedidos possui IDs sem reutilização e armazenamento próprio
por `Arc`; nenhuma mensagem contém ponteiros do chamador. Timeout de 1 segundo
cancela o pedido se ainda não começou. Pedidos já em execução mantêm sua memória.
Callbacks reentrantes são recusados para os comandos. Uma referência adicional
ao módulo protege o procedimento instalado. A remoção acontece na thread da
janela e a referência só é liberada depois do retorno síncrono e com zero
callbacks ativos. `client_stop` remove o dispatcher; o controlador recusa
`FreeLibrary` quando a parada não conclui. Se outra biblioteca assumir a janela,
ou ela for destruída/travar, a referência permanece e pode exigir reabrir o jogo.
Esse comportamento prioriza não deixar callbacks apontando para uma DLL removida.

**Validação:** 16 testes nativos, 13 testes x86 sob Proton, Clippy nativo/x86 e
formatação. O teste entre processos usa duas janelas Rust próprias (ANSI e
Unicode), DLL real, execução na thread correta, encaminhamento de texto e ping,
timeout durante bloqueio, ausência de execução tardia do pedido cancelado,
restauração, descarga e nova carga limpa. Não depende de assets comerciais.

Na partida aberta pela Steam, o novo processo Windows 412 usou janela `0x60098`
e thread 416; `probe`, `registry` e `inspect actors.car` executaram na thread 416.
Foram lidos os 80 tipos e tamanho `0x2118`, construtor `game.dll+0xcef70` e
destrutor `game.dll+0xd1820`. `detach` retornou `active=0`, `inflight=0`; a DLL
foi descarregada. PIDs/handles são somente evidência desta sessão.
Uma tentativa anterior instalou o dispatcher no PID 420, mas o processo encerrou
antes da primeira consulta. O proprietário não relatou fechamento manual;
nenhum diagnóstico de crash foi obtido. A repetição após abrir pela Steam passou;
a causa da primeira saída continua indeterminada e exige observação prolongada.
Os seis hashes do manifesto de originais continuaram iguais após os testes.

**Limite e próximo bloco:** o dispatcher comprova a thread da janela, não a fase
segura do motor. A inspeção de `prism3d.exe` aponta uma rotina que executa tarefas
e depois processa mensagens, mas ainda há caminhos reentrantes de carregamento
a investigar. Nenhum construtor foi chamado. A rotina de factory em `p3shared`
incrementa contadores em uma cadeia (`+0x31f20`); a remoção os decrementa
(`+0x31f40`). Não tratar isso como mutex nem copiar objetos em memória. Confirmar
fase, ABI, contadores e ownership antes de criar/remover uma entidade privada;
só depois integrar modelo, mundo e transform. Ainda não há veículo remoto visual.

### M4 — Sonda do ponto de retorno do loop principal — 27/10/2026

`engine trace` captura até 32 frames com `RtlCaptureStackBackTrace`. Na bancada
retorna frames; na partida Proton, o unwind terminou nas rotinas Win32/ntdll,
sem alcançar o chamador do executável. Não foi usado para presumir uma fase.

Foi implementada em Rust uma sonda x86 opcional na entrada IAT de `PeekMessageA`
do executável. `engine pump-attach` exige os hashes dos três módulos, entrada
original sem hooks de terceiros e execução do controle na thread da janela.
O perfil acrescenta SHA-256 de `prism3d.exe`
`949be316b38eaf4d80d4f7f6a3e7ae040e6c6072c92267b3d10066140058f61e`,
IAT `+0x22188` e retornos `+0x1473b`, `+0x1476e`, `+0x147a4`. O shim
`naked_asm!` captura o retorno antes do prólogo, realinha a stack para Rust,
encaminha os cinco argumentos stdcall e preserva o resultado e `GetLastError`.
Depois de `PeekMessageA` retornar, conta somente os pontos do perfil na thread
esperada. Não executa construtores, filas de entidades ou comandos de rede.

A entrada é trocada por compare/exchange alinhado, com restauração da proteção
da página. A sonda é removida por `engine pump-remove` ou automaticamente por
`stop`, na thread da janela. **O uso da sonda fixa a DLL até o fim do processo**:
restaurar o import não prova que outra thread não tenha carregado seu valor
anterior. Não se tenta descarregar código enquanto essa possibilidade existir.
O controlador informa essa condição antes de chamar `FreeLibrary`. É necessário
encerrar o jogo antes de reconstruir a DLL. Esse limite é específico da sonda;
o dispatcher sem sonda continua permitindo carga/descarga completas.

A ABI de diagnóstico evoluiu para v2/604 bytes; requer recompilar launcher e
DLL juntos. A ABI v1 de configuração/rede permanece inalterada. Pedidos da
extensão com a versão antiga são recusados antes de acessar o restante do bloco.

**Validação automatizada:** 16 testes nativos; 14 testes x86 sob Proton;
Clippy nativo/x86 e formatação. Um teste x86 chama o shim mil vezes com cinco
argumentos diferentes, confere retorno/stack e erro Win32. A integração usa uma
terceira janela Rust ANSI com loop PeekMessage e uma entrada própria numa página
somente leitura; verifica observações na thread correta, remoção, interrupção
dos contadores, proteção restaurada, recusa de descarga e funcionamento da
janela até sua saída. Os cenários anteriores ANSI/Unicode continuam passando.

**Validação na partida:** PID 412/thread 416, **3.414.859 retornos** observados
antes da remoção, incluindo `prism3d.exe+0x1476e` e `+0x1473b`. Com a sonda
ativa, o proprietário dirigiu, voltou ao menu e carregou novamente o save de
teste; confirmou funcionamento normal, sem travamento. `pump-remove` retornou
estado 2. A leitura externa da IAT confirmou o ponteiro restaurado em USER32
(`0x7ba17d00` nesta sessão). Após `stop`, `active=0`, `inflight=0`, status 2;
o total permaneceu exatamente 3.414.859. `unload` recusou a descarga com o
diagnóstico previsto de módulo fixado, antes de chamar `FreeLibrary`.
O jogo continuou aberto; a DLL está parada, sem sonda/procedimento instalados,
mas residente até encerrar esse processo. Os seis hashes do manifesto de
originais foram conferidos novamente e continuaram iguais. Não houve escrita
em salvamentos pelas ferramentas. A primeira saída indeterminada registrada no
bloco anterior continua sem causa determinada; esse cenário não a reproduziu.
O contador de referências do contexto em `p3shared.dll+0x51314 -> +0x8` foi
somente lido (1845 nesta amostra); não se presume que seja uma trava.

**Próxima dependência:** correlacionar esse ponto com os estados de carregamento
e comprovar o contrato de lifetime dos factories. Para executar entidades,
adicionar uma fila própria até esse ponto do loop e validar ABI/ownership em
fixture antes da criação/remoção privada. A fila de janela atual continua sendo
somente de diagnóstico. M4 não está concluído e não há caminhões remotos.

### M4 — Fila do loop e lifetime privado real — 27/10/2026

Entregues `phase_queue`, `engine_contract`, `engine_lifecycle`, fixture de
factories x86 e comandos `engine phase-probe` / `engine private-cycle [--cycles N]`.
O diagnóstico usa ABI v3/664 bytes, com recusa dos blocos v1/v2; configuração e
rede preservam a ABI v1. Não foi criada uma API de criação a partir de pacotes UDP.

**Fase e fila:** no máximo 16 pedidos pertencentes à DLL, uma operação por
passagem, somente na thread da janela e nos retornos do perfil. A profundidade
de `PeekMessageA` deve ser 1 e nenhum callback da janela pode estar ativo.
`WM_QUIT` tem prioridade. Timeout pendente cancela antes da execução; timeout
com trabalho iniciado devolve diagnóstico sem liberar o pedido ou interromper
o motor. A parada cancela pendências e recusa desmontagem se houver trabalho
ativo. A fixação do módulo introduzida pela sonda continua exigindo encerrar
o processo antes de reconstruir a DLL.

**Contrato de alocação:** a criação usa o export `cdecl` de `p3shared.dll+0xcba0`,
registro `game.dll+0x127a34`, variante `false` e armazenamento nulo. A remoção
usa exclusivamente o export pareado em `p3shared.dll+0xcc40`. Um guard Rust
destrói o objeto na mesma operação, inclusive após falha de validação Rust.
Nenhum objeto ou ponteiro proprietário é mantido entre frames. São conferidos
os hashes dos três módulos, exports, callbacks executáveis, registro, tamanho,
vtable e destrutor gravado em `objeto+0x58`.

A análise adicional identificou o contexto do jogo em `game.dll+0x160a24`:
o wrapper do construtor/destrutor também chama `vprocess_t::lock/unlock`.
Essas funções incrementam/decrementam referências, incluindo os ancestrais.
`engine_contract` captura as duas cadeias, rejeita ponteiros nulos, ciclos,
overflow, troca de cabeça/pai e contagens inesperadas. Ancestrais compartilhados
recebem o incremento de cada cadeia; não se presume delta 1 para todos os nós.
Os dois contextos precisam voltar exatamente ao estado inicial de cada ciclo.
A posição local é lida antes e depois, e precisa continuar igual durante o
comando. Não são chamados `init`, inserção, carregamento de modelo ou física.

**Bancada:** 19 testes nativos; 17 testes x86 sob Proton; Clippy nativo/x86 e
formatação passaram. O host Rust próprio possui factories `cdecl`, booleano e
armazenamento explícitos, objeto com destrutor em `+0x58`, métodos virtuais
`thiscall`, checagem da thread, rastreamento de alocações e dois contextos com
ancestral compartilhado. Quatro ciclos passaram; um erro intencional de vtable
também liberou a alocação e restaurou os contadores. A fixture bombeia mensagens
dentro de um callback: o pedido de criação expira sem executar depois e sem
alocação adicional. Foram verificados timeout após início, recuperação de panic
Rust, limite de fila, cancelamento na parada e restauração da página/entrada.
Nenhum teste automatizado precisa de assets comerciais.

**Teste real no jogo original sob Proton:** PID Windows 396/thread 400, com o
caminhão parado em save de teste. `phase-probe` e três comandos de lifecycle
concluíram, com fila vazia e nenhuma operação ativa ao final:

| Cenário | Ciclos | Contador do motor antes/criado/depois |
| --- | ---: | --- |
| Primeiro objeto privado | 1 | 1865 / 1866 / 1865 |
| Repetição, sempre um objeto por vez | 16 | 1848 / 1849 / 1848 no último ciclo |
| Após dirigir, salvar manualmente, menu, carregar e dirigir novamente | 1 | 1882 / 1883 / 1882 |

Foram **18 alocações/destruições reais de `actors.car`**, tamanho `0x2118`,
vtable `game.dll+0x1257a0` e destrutor `game.dll+0xd1820`. As referências do
contexto do jogo e seus ancestrais também passaram nas verificações. O valor de
repouso muda com a atividade normal do jogo; o equilíbrio é conferido dentro de
cada ciclo. A posição local permaneceu igual em cada comando. O proprietário
conseguiu dirigir, salvar e recarregar entre os testes; não relatou falha.

O prefixo/saves anteriores foram copiados com o jogo fechado para backup local.
O proprietário salvou durante o procedimento; os saves após essa ação também
foram preservados, e seus hashes continuaram iguais após a última operação.
As ferramentas não escreveram saves. Os seis arquivos do manifesto de originais
continuaram iguais. `pump-remove` restaurou a entrada, `stop` confirmou parada
e remoção do procedimento da janela; o módulo permaneceu residente conforme a
política da sonda. Evidências e backups permanecem em `target/`, fora do Git.

**Limites:** o objeto é privado, não inicializado e destruído imediatamente.
Isso não cria um caminhão visível nem prova inserção/remoção no mundo. As
checagens de contadores não são uma auditoria completa do heap. O guard cobre
fluxos de erro/panic Rust, não exceções nativas C++/SEH. O resultado vale somente
para o perfil e ambiente registrados.

**Próxima entrega:** mapear os valores de entidade e os contratos de `c_item_t`,
`entity_proc_t::init_eblock` e inserção/remoção, identificar configuração de
modelo e transform e criar um visual mínimo sem física/colisão. A inspeção
somente leitura encontrou a raiz de valores de `actors.car` em
`game.dll+0x15fdb8`; os nós são inicializados em runtime. Decodificar esses
metadados com limites antes de chamar setters ou inicialização. Não copiar o
objeto do jogador para simular outro veículo.

### M4 — Metadados de entidade e definições de assets — 27/10/2026

Entregues o decodificador portável `engine_values`, `memory_tool values --pid`,
`engine_report string --rva` e `asset_tools read ARQUIVO ENTRADA --max-kib N`.
São consultas somente leitura: nenhuma DLL é carregada e nenhum callback do
motor é executado. A nova sessão foi aberta pela Steam, e o proprietário
carregou a partida. O PID foi enumerado novamente; os hashes de ambos os módulos
passaram. A raiz runtime `game.dll+0x15fdb8` retornou sete nós:

| Propriedade | Código observado | Offset no objeto | Callback de escrita em p3shared.dll |
| --- | ---: | --- | --- |
| `truck_build` | 14 | `+0x12c8` | `+0x1b10` (`me_string_set`) |
| `useable` | 2 | `+0xf4` | `+0x1070` (`me_bool_set`) |
| `rotation` | 9 | `+0xe0` | `+0x1400` (`me_point_set`) |
| `position` | 9 | `+0x94` | `+0x1400` (`me_point_set`) |
| `volume` | 8 | `+0xb8` | `+0x16c0` (`me_real_set`) |
| `rid` | 5 | `+0x64` | `+0x1970` (`me_s16_set`) |
| `name` | 14 | `+0x4` | `+0xa0f0` (callback próprio da entidade) |

Os nomes dos exports identificam os callbacks, sem constituir autorização para
chamá-los. A implementação retém as 24 palavras por nó sem inferir o significado
de flags/tipos desconhecidos. Limita nós/nomes, verifica alinhamento, overflow,
terminação/UTF-8, ciclos e memória ilegível, e relê links/nome-pointers ao final.
Isso detecta mudanças estruturais, não torna toda a captura atômica.

A inspeção do caminhão local identificou `c_item_t` em
`game.dll+0x160df0->0x14c->0x4610->0x8->0x0`; seu `+0x8` aponta para o
`actors.car`. Foram conferidos vtable/destrutor do perfil, backlink do objeto
em `+0x60`, registro do item em `+0x34` e tamanho `+0x48 = 0x2118`.
`truck_build` tinha terminador inicial vazio e a posição declarada estava zerada,
apesar de o caminhão estar carregado e da cadeia da física continuar disponível.
Logo, esses metadados não bastam para transmitir o transform atual ou reconstruir
o modelo de um save. A convenção de rotação continua pendente.

Os SCS foram lidos diretamente, sem extração: as definições de tráfego referem
modelos/shaders; os caminhões configuráveis usam definições de acessórios,
incluindo base e rodas. Esse conteúdo não foi incorporado aos fontes. A leitura
é limitada a 1 MiB, compara tamanho declarado/real e rejeita entradas ambíguas,
inválidas ou não textuais. O helper PE imprime strings escapadas e limitadas.

**Validação:** 22 testes nativos e 19 testes x86 sob Proton, incluindo os novos
casos de decoder e texto SCS com fixtures próprias; build cruzado release,
Clippy nativo/x86 e formatação passaram. A integração de janela, fila e factories
continuou passando na bancada. Não houve criação ou escrita no jogo neste bloco.

**Próximo contrato:** o contêiner do motor tem ownership separado do objeto:
`alloc_eitem` chama `prism3d!mm_sys_get_new_anitem` e instala a tabela de operações
em `p3shared.dll+0x4523c`. O caminho de liberação chama `entity_destroy(obj,obj)`,
com armazenamento explícito, e depois libera o bloco do item. Não substituir esse
fluxo por `entity_destroy(obj,null)` usado no ciclo privado independente. Comprovar
a alocação/fixação, referências, callbacks e liberação do contêiner em fixture
antes da inicialização/inserção. Não chamar `init_eblock` sobre um objeto avulso.

### M4 — Ownership do contêiner e armazenamento explícito — 27/10/2026

Entregues `engine_item` e `engine item-cycle [--cycles 1..16]`, sem alterar o
layout da ABI v3/664 bytes nem a configuração/rede v1. O ciclo executa na mesma
fila do loop validada anteriormente e mantém um único item/ator privado por vez.
Não chama inicialização, inserção, física, render ou criação de HUD.

**Contrato específico do perfil:** `entity_proc_t::alloc_eitem` em
`p3shared.dll+0xc9a0` usa a ABI `thiscall`; os helpers de armazenamento/referência
em `prism3d.exe` são `cdecl`. A tabela de nove palavras em
`p3shared.dll+0x4523c` é verificada integralmente, incluindo callbacks preenchidos
em runtime. O item vem do pool anônimo com flags `0xd2`, uma referência e sem
objeto, pai, filhos, armazenamento ou fixação. A DLL grava o registro próprio do
tipo no item e chama `ci_alloc_mem_fix` (`prism3d.exe+0x13420`): armazenamento do
tamanho do ator, uma fixação e bit de memória carregada.

O factory já validado recebe a variante `false` e o endereço desse armazenamento,
construindo o ator no mesmo endereço. São preenchidos backlink `objeto+0x60`,
contexto `+0x84` e seletor `+0x50`, conforme o caminho de criação observado no
motor. Nenhum desses campos é copiado do caminhão do jogador. Vtable, destrutor,
ambas as cadeias de contadores e bytes do allocator precisam bater com o contrato.

Um guard distingue item vazio, armazenamento sem objeto e objeto construído.
Solta somente a fixação exclusiva adquirida pela operação e devolve a referência
com `ci_dec_refs` (`prism3d.exe+0x13520`). Para um objeto construído, o callback
do item em `p3shared.dll+0xa9a0` chama `entity_destroy(objeto,objeto)` e depois
libera o armazenamento. A DLL não chama o destrutor independente com argumento
nulo sobre essa memória. Antes da construção, a limpeza usa `ci_free_mem` sem
destruir memória não inicializada. Se a fixação não for mais exclusiva, o guard
retém o item e a checagem de retorno ao pool falha; não tenta liberar memória
com outro proprietário. Exceções nativas C++/SEH continuam fora dessa proteção.

Após cada operação, o item precisa voltar ao pool com flags `0x80`, sem ponteiro,
tamanho, referências ou fixações. Os totais u64 do manager em `+0x60`/`+0x68`
precisam avançar exatamente `0x2118` bytes alocados/liberados. Isso comprova o
equilíbrio do armazenamento observado, não de todo o heap do processo.

**Bancada:** 22 testes nativos e 19 testes x86 sob Proton passaram; Clippy
nativo/x86, formatação e build release passaram. O host próprio agora separa
pool, armazenamento e objeto, aceita construção no endereço fornecido e recusa
destruição/liberação erradas. O teste de integração conferiu quatro ciclos
normais, erro de cabeçalho após construção, falha de alocação e construtor nulo:
sete itens devolvidos, seis buffers alocados/liberados, cinco atores construídos/
destruídos pelo caminho do contêiner. A criação independente também continuou
passando. Um pedido de item durante callback aninhado expirou sem executar.

**Jogo original sob Proton:** nova sessão, PID Windows enumerado 396, execução
na thread 400. Nenhuma sonda estava carregada antes do build deste bloco.

| Cenário | Ciclos | Referências do motor antes/criado/depois |
| --- | ---: | --- |
| Primeiro contêiner com ator | 1 | 1864 / 1865 / 1864 |
| Repetição individual, sem objetos simultâneos | 16 | 1864 / 1865 / 1864 no último ciclo |
| Após dirigir, menu e carregar novamente | 1 | 1852 / 1853 / 1852 |

Foram **18 ciclos reais com contêiner**, além dos 18 ciclos independentes do
bloco anterior. Em todos, retornaram as referências do jogo/motor, item limpo no
pool e `8472` bytes de armazenamento liberados; posição local igual dentro de
cada comando. O proprietário confirmou movimento/recarga normais. Os hashes do
save preservado e dos seis originais continuaram iguais. `pump-remove` restaurou
a entrada, e `stop` removeu o procedimento e confirmou parada. A DLL continua
fixada até o processo encerrar; não reconstruí-la enquanto o jogo estiver aberto.

**Próxima dependência:** selecionar o caminho de um visual sem HUD/controle/IA/
colisão antes de chamar inicialização. A lista de init de `actors.car` em
`game.dll+0x14d1b8` inclui níveis `-1` e `10`; o método em `+0x50f40`, nível 10,
cria `/ui/ghud.esc`. Portanto, inicializar indiscriminadamente um segundo
`actors.car` pode acionar comportamento de jogador local. Investigar o caminho
de modelo/render ou um tipo visual já disponível (por exemplo `decals.pmd`),
com teardown próprio, sem chamar o init completo nem clonar o objeto local.
Não há ainda um caminhão remoto visível.

### M4 — Perfil do candidato visual `decals.pmd` — 27/10/2026

O perfil passou a descrever tipos explicitamente, com uma lista permitida de
`actors.car` e `decals.pmd`. `memory_tool values --entity` seleciona metadados;
`engine item-cycle --kind car|pmd` seleciona somente o ciclo privado. Nome
desconhecido é recusado; não é possível fornecer RVAs/callbacks arbitrários ao
comando do jogo. O layout das ABIs foi preservado. A criação independente antiga
continua restrita ao carro. O novo tipo é candidato a visual; o perfil não
comprova seu contrato de init/render.

Contrato de `decals.pmd`: registro `game.dll+0x12a318`, tamanho `0xa8c` (2700),
construtor false `+0xf9c70`, destrutor `+0xf9980`, vtable `+0x12ac68`, metadados
runtime `+0x15bd00`. A inicialização do módulo instala em `+0x1665e4`/`+0x1665e0`
os callbacks de lock/unlock `+0xcedf0`/`+0xcee00`; esses ponteiros também são
verificados antes do ciclo. Assim a cadeia do contexto do jogo validada no carro
permanece aplicável a esse wrapper, sem presumir isso para qualquer tipo futuro.

O decoder retornou **13 definições**, sendo sete novas em relação à base comum:

| Propriedade do PMD | Offset | Tamanho de elemento observado |
| --- | --- | ---: |
| `model_name` | `+0x200` | 64 |
| `model_skins` | `+0x240` | 64, limite inline 16 |
| `model_rotation` | `+0x640` | 12 |
| `model_scale` | `+0x64c` | 12 |
| `play_animation` | `+0x658` | 1024 |
| `rand_rotation` | `+0xa58` | 3 |
| `clip2plane` | `+0xa5b` | 1 |

As demais são `useable`, `rotation`, `position`, `volume`, `rid` e `name`.
Os metadados continuam sendo observações, não uma API pública de escrita. A
análise de `me_gen_insert` corrigiu dois rótulos no relatório JSON: `+0x18` é
`element_size`; `+0x1c` é `table_bound`, capacidade inline ou parâmetro de
crescimento da tabela indireta. O antigo nome `table_offset` não descrevia esse
campo. O offset da contagem de `model_skins` é `+0xa5c`. Não escrever escala,
rotação ou array sem validar os callbacks e suas regras de atualização.

Entregue ainda `engine_report field --offset`: consulta nativa de acessos a
membros em memória baseada em registradores, excluindo imediatos e referências
absolutas. A fixture própria cobre essa distinção. Uma ocorrência continua
podendo pertencer a outro tipo ou a dados decodificados como código; consultar
o contexto antes de usar os resultados como evidência.

**Validação:** 23 testes nativos e 19 x86 sob Proton, build release, Clippy
nativo/x86 e formatação passaram. Na nova partida foi enumerado o PID Windows
396; hashes, registro, callbacks e thread 400 conferidos. Um ciclo PMD e depois
16 ciclos passaram, sempre um item/objeto por vez, com referências
`1826 / 1827 / 1826`, item limpo no pool e 2700 bytes alocados/liberados por ciclo.
A posição local permaneceu igual em cada operação. A sonda foi restaurada,
`stop` removeu o procedimento, fila vazia, nenhuma operação ativa e contador
parado em 666181 hits. Save e seis originais mantiveram os hashes. O proprietário
confirmou dirigir/menu/recarga e encerrou o processo após o teste.

**Limite/próximo bloco:** nenhum PMD foi inicializado ou inserido. Não há ainda
modelo carregado, veículo visível ou contrato de colisão/transform comprovado.
Validar setters em fixture, configuração de um modelo original disponível,
init/destruição e ownership dos recursos antes de manter um objeto entre frames.
O arquivo local `models/ai/truck/models/truck.psm` é um candidato existente no SCS;
o projeto não transporta esse asset nem presume que ele carrega sozinho.

## 3. Primeira versão jogável e limites

O primeiro marco de produto é **dois jogadores em computadores distintos na LAN,
na mesma versão/mapa, vendo os caminhões um do outro se moverem com posição e
orientação coerentes**. Entrada, saída e reconexão devem funcionar sem deixar
entidades abandonadas ou alterar saves por acidente.

Para chegar a esse marco, começar com caminhões remotos visuais sem colisão.
Cada cliente mantém a física do seu caminhão; o servidor distribui e valida o
estado recebido. Este é um modelo de autoridade inicial, não uma simulação
física autoritativa completa. Colisão entre jogadores depende de decisões e
trabalho adicionais descritos adiante.

Economia, missões, trânsito, pedestres e demais sistemas permanecem locais até
receberem regras explícitas de sincronização. O produto deve informar essa
limitação. Não anunciar mundo compartilhado completo apenas por haver veículos
visíveis. Internet pública, contas e matchmaking ficam depois da LAN funcional.

## 4. Arquitetura e decisões a preservar

- `asset_tools`: pipeline de assets, inspeção e preparação de pacotes próprios.
- `multiplayer_server`: sessão de rede e protocolo compartilhado, atualmente no
  mesmo crate com features que permitem usar o protocolo sem iniciar o servidor.
- `client_dll`: ponte com o jogo, captura local, aplicação de estados remotos,
  worker e binários auxiliares Rust. Um launcher pode começar em `src/bin/`.
- A integração do motor deve ser isolada da rede. Receber um datagrama não deve
  executar diretamente uma chamada desconhecida do motor na thread UDP.
- Perfis precisam identificar a versão do módulo por hash, arquitetura,
  estruturas e capacidades validadas. Nome Steam sozinho não identifica bytes.
- Configuração deve permitir caminhos externos ao repositório, servidor e
  perfil explícitos. Caminhos absolutos desta máquina não são configuração pública.
- Manter `DllMain` mínimo. Inicialização/parada, joins e descarregamento precisam
  de ciclo explícito fora do loader lock.
- Se a complexidade justificar separar protocolo, integração ou bootstrap em
  novos crates, fazê-lo com dependências claras e atualização da documentação.

## 5. Etapas e critérios de conclusão

### M0 — Continuidade, reprodutibilidade e proteção dos originais

- [x] Criar workspace, lockfile, README e exclusão do jogo por padrão.
- [x] Registrar roadmap e autonomia em arquivos versionáveis.
- [x] Fixar um toolchain testado em `rust-toolchain.toml`, após verificar os
  componentes necessários para x64 e x86; preservar o `Cargo.lock`.
- [ ] Criar preparo/diagnóstico de ambiente em Rust, com caminhos detectados,
  sem depender de um script de outra linguagem nem sobrescrever configuração
  Cargo existente sem mesclagem explícita.
- [ ] Tornar o desenvolvimento fora da pasta Steam o fluxo principal: caminho
  do jogo configurável e saídas de build no repositório de fontes.
- [x] Inicializar Git e vincular o remoto indicado pelo proprietário, preservando
  o commit inicial de licença e auditando os arquivos próprios antes do envio.
- [x] Adotar Apache-2.0, conforme a licença criada pelo proprietário no remoto.
- [ ] Definir política para contribuições e revisar as licenças das dependências
  e os avisos que acompanham releases.

**Concluído quando:** um clone limpo em outro Windows compilar as ferramentas e
a DLL x86 usando apenas instruções/arquivos versionados, sem copiar o jogo do
primeiro computador ou o seu `target/`.

Para o PC Linux de continuidade, incluir também build/testes nativos das partes
portáveis e compilação cruzada da DLL. Registrar esses resultados separadamente
da execução no jogo; não considerar um build cruzado prova de integração funcional.

### M1 — Perfil do jogo e leitura de estado confiável

- [x] Encontrar uma cadeia e validá-la contra save após reinício.
- [x] Transformar a descoberta em perfil nomeado, com hash e diagnóstico de
  versão incompatível; não aplicar offsets por semelhança do nome do arquivo.
- [x] Resolver a base do módulo no processo alvo e cada cadeia por sessão.
- [ ] Identificar estados em que veículo/módulos ainda não existem. Suspender
  amostragem durante menus, carregamento e troca de veículo sem matar o jogo.
- [ ] Ler posição/orientação como um estado consistente: agrupar leituras quando
  contíguas e investigar captura no tick do jogo ou verificação de consistência.
  Uma cópia contígua sozinha não garante atomicidade frente à física.
- [ ] Confirmar ordem dos eixos, escala, quaternion, velocidade linear/angular,
  direção de marcha e identificador do modelo. Adicionar só campos comprovados.
- [ ] Testar câmera diferente, pausa, troca de caminhão, reboque, nova partida,
  carregamento de outro save e uma segunda instalação com o mesmo hash.

**Concluído quando:** o perfil lê o estado correto, explica indisponibilidade
sem inventar posições e passa por toda a matriz de estados documentada.

### M2 — Carregar e controlar a DLL x86 no jogo

- [x] Implementar launcher/carregador em Rust, com alvo explícito, DLL por
  caminho absoluto e verificações de arquitetura/perfil.
- [x] Escolher e documentar uma estratégia de carregamento; usar apenas o
  mecanismo necessário. Resolver exports e módulos no processo correto,
  sem transportar endereços locais de funções para outro processo por hipótese.
- [x] Definir API de configuração com versão/tamanho de estrutura e ABI estável.
  Não depender de alterar variáveis de ambiente de um jogo já iniciado pela Steam.
- [x] Iniciar após o carregamento e fora de `DllMain`; evitar início duplicado.
- [x] Expor estado, contadores e último erro de forma consultável. Logs locais
  devem indicar modo real/simulado e perfil selecionado.
- [x] Encerrar worker antes de descarregar; tratar DLL já carregada e início
  duplicado. Não há referências persistentes a entidades do motor nesta etapa.
- [ ] Ampliar validação de saída do jogo, falha de rede prolongada e
  inicialização incompleta. Cobrir referências ao motor quando M4 as introduzir.
- [x] Separar falha de leitura temporária de uma versão realmente incompatível.
- [x] Validar o ciclo em host Rust próprio e depois no processo original, com
  dados reais enviados e recebidos por outro cliente.

**Concluído quando:** é possível iniciar e parar o cliente no jogo real de forma
repetível, observando pacotes reais, sem modificar executáveis originais.
Se algum método exigir alteração de arquivo, documentar e testar restauração.

### M3 — Sessão LAN e protocolo de estado completo

- [x] Ter UDP, alvo de 60 Hz, IDs provisórios, limites e descarte de sequências antigas.
- [ ] Adicionar negociação de versão, criação/entrada de sala e aceite/recusa.
- [ ] Definir IDs de sessão/jogador e reinício de sequência por conexão. O
  endereço UDP atual não deve ser a identidade persistente do jogador.
- [ ] Definir mensagens de entrada, saída, heartbeat, timeout e reconexão.
- [ ] Acrescentar rotação, tick/timestamp, modelo e velocidade com formatos e
  unidades documentados. Tratar compatibilidade e valores inválidos.
- [ ] Separar eventos que exigem confirmação/reenvio de snapshots descartáveis.
  Definir IDs, deduplicação e prazos para eventos confiáveis sobre UDP.
- [ ] Medir RTT, estimar diferença de relógios e ordenar snapshots para interpolação.
- [ ] Manter tamanho de datagrama controlado, evitar fragmentação e limitar filas,
  clientes, taxa de pacotes e alocação por origem.
- [ ] Adicionar métricas de pacotes, atraso, perda, jitter e tick atrasado.
- [ ] Medir custo do broadcast atual, que cresce com pares de clientes; avaliar
  agrupamento, frequência e relevância espacial quando os números justificarem.
- [ ] Testar duas máquinas reais; configurar bind LAN, IP e porta. A configuração
  atual `127.0.0.1:39000` atende somente o computador local.

**Concluído quando:** dois clientes entram, trocam posição/rotação e reconectam,
com incompatibilidades diagnosticadas e comportamento definido sob perda/reordenação.
60 Hz é a cadência alvo de envio, não garantia de chegada nem taxa de renderização.

### M4 — Criar e remover caminhões remotos no motor

Esta é a maior incerteza técnica do caminho até o primeiro multiplayer visual.
O factory privado foi identificado e validado. Inicialização e integração no
mundo ainda precisam de contratos e testes próprios.

- [ ] Investigar criação de veículo/modelo, atualização de transform e remoção,
  usando somente a instalação local e registrando conclusões próprias.
- [x] Descobrir o registro de tipos e consultar 80 entradas na partida.
- [x] Decodificar a cadeia de valores de `actors.car` e consultar as sete
  definições herdadas na partida sem chamar getters/setters.
- [x] Perfilar metadados e lifetime privado de `decals.pmd` como candidato visual;
  13 propriedades lidas e 17 ciclos com contêiner passaram no jogo.
- [x] Observar retornos exatos do loop principal na thread da janela, incluindo
  movimento/menu/recarga; restaurar a instrumentação e conferir a parada.
- [ ] Comprovar a thread/fase e a propriedade das operações de inicialização e
  integração no mundo; o contrato do ciclo privado já passou.
- [x] Confirmar ABI x86 dos factories e lifetime privado: convenções de chamada,
  variante, armazenamento, tamanho, alocação e liberação pareadas.
- [x] Validar item anônimo, armazenamento explícito e retorno ao pool em host
  próprio e na partida, incluindo recarga e equilíbrio de bytes/referências.
- [ ] Confirmar ABI/ownership de inicialização, inserção, transform e remoção
  do mundo antes de ampliar as chamadas `unsafe`.
- [x] Criar fachada Rust para o ciclo privado, mantendo offsets e chamadas
  do motor fora do núcleo de rede; ampliar somente com operações comprovadas.
- [x] Encaminhar diagnóstico e lifecycle por fila limitada até o ponto observado
  do loop, fora de callbacks/reentrância; nenhum objeto é manipulado pelo UDP.
- [ ] Criar primeiro um único veículo visual de teste a distância controlada,
  usando assets originais disponíveis localmente e física/colisões desativadas.
- [ ] Mapear jogador → entidade, atualizar transform, remover na saída e limpar
  referências ao carregar outro mapa/save ou encerrar a sessão.
- [ ] Confirmar que mudar o remoto não muda o caminhão local nem os dados do save.
- [ ] Usar overlay/indicador apenas como diagnóstico se necessário; isso não
  substitui a entrega de um caminhão remoto no mundo 3D.

**Concluído quando:** dois jogos em máquinas distintas mostram a entidade remota
correta e ela aparece/desaparece com a sessão sem duplicatas ou referências inválidas.

### M5 — Movimento remoto suave e apresentação

- [ ] Manter buffer por jogador com timestamps e limite de memória.
- [ ] Interpolar posição e orientação; confirmar convenção de quaternion e tratar
  rotações equivalentes para não girar pelo caminho longo.
- [ ] Definir atraso de interpolação, extrapolação curta e comportamento quando
  os dados param de chegar. Medir em vez de escolher tolerâncias cegamente.
- [ ] Separar teleporte/correção de movimento normal; não atravessar o mapa com
  interpolação longa após reconexão ou carregamento.
- [ ] Sincronizar modelo/cabine, reboque e sinais visuais conforme descoberta:
  direção, rodas, freio, faróis e setas, sem exigir todos para o primeiro teste.
- [ ] Tratar visibilidade, LOD, nomes e estados de desconexão sem poluir a interface.
- [ ] Medir uso de CPU, memória, rede e efeito no FPS com dois e mais jogadores.

**Concluído quando:** dirigir e observar curvas/frenagens na LAN é coerente e
suave, e perda temporária de pacotes não produz movimentos indefinidos.

### M6 — Estado do mundo e regras de gameplay

- [ ] Negociar mapa, versão, mods e ponto de entrada compatíveis antes de criar
  remotos; impedir entrada em mundos incompatíveis com mensagem útil.
- [ ] Definir relógio, clima, pausa e carregamento durante a sessão.
- [ ] Definir spawn que evite sobreposição e respeite a disponibilidade do mapa.
- [ ] Escolher dono e ciclo de anexar/desanexar reboques; eventos devem ser idempotentes.
- [ ] Definir a política de trânsito: desativado no teste controlado, local com
  limitações explícitas ou sincronizado por um responsável. Não supor que duas
  simulações locais terão os mesmos veículos e colisões.
- [ ] Especificar colisão entre jogadores e correções físicas. Avaliar autoridade
  de host/servidor, predição e reconciliação antes de habilitar contato físico.
  Um servidor Rust de relay não possui automaticamente a física do jogo.
- [ ] Definir entregas, cargas, dinheiro, multas, danos e progresso: primeiro
  preservar economia local; compartilhar somente com regras e persistência próprias.
- [ ] Isolar dados multiplayer dos saves de uso normal, com backup/restauração
  e tratamento de desconexão durante eventos importantes.

**Concluído quando:** as regras escolhidas estão implementadas e testadas; a
interface informa claramente quais sistemas são compartilhados e quais são locais.
Colisão e economia compartilhadas podem ser marcos posteriores ao MVP visual.

### M7 — Pipeline de assets e modificações distribuíveis

- [x] Ler os três SCS locais, extrair `data.scs`, converter uma TGA para PNG e
  reempacotar 76 arquivos num pacote separado de teste.
- [ ] Organizar entradas originais, edição própria, cache e saída de build em
  caminhos distintos; registrar origem e hash em manifesto gerado.
- [ ] Criar builds incrementais/determinísticos quando viável, com dependências
  explícitas de texturas, materiais e caminhos internos.
- [ ] Preservar canais de cor, transparência, orientação e resolução; adicionar
  codecs/mipmaps apenas quando um asset real exigir e houver validação visual.
- [ ] Definir sobreposição de pacotes e um instalador/restaurador em Rust que
  não sobrescreva silenciosamente originais ou alterações do usuário.
- [ ] Testar um pacote mínimo no jogo, registrar o resultado e testar sua remoção.
- [ ] Distribuir código, assets próprios e manifestos; reconstruir localmente o
  que depende de conteúdo comercial. Não publicar o `base.scs` reempacotado.
- [ ] Criar auditoria Rust que impeça inclusão acidental de originais em artefatos.

**Concluído quando:** uma alteração própria pode ser construída, instalada,
verificada e removida em outra instalação original sem distribuir o jogo.

### M8 — Robustez, testes e observabilidade

- [ ] Cobrir parsing/limites, protocolo, mudança de versão, sessões e interpolação
  com fixtures próprias e testes que verifiquem comportamento relevante.
- [ ] Simular perda, duplicação, jitter, atraso e reordenação com uma ferramenta
  Rust controlada; registrar parâmetros e resultado reproduzível.
- [ ] Testar sessão longa, menus, reload, encerramento abrupto e reconexão, com
  verificações de vazamento e remoção de entidades.
- [ ] Ter logs com versão/perfil/estado e coleta opcional de métricas, sem dumps
  completos ou conteúdo do jogo nos relatórios públicos.
- [ ] Configurar CI de formatação, Clippy, testes e build x86. Builds de integração
  com o jogo continuam locais; CI não deve baixar assets comerciais.
- [ ] Auditar parsers e caminhos de arquivo/extração; limitar origem de pacotes,
  filas e mensagens. Downloads/executáveis recebidos da rede não fazem parte do protocolo.
- [ ] Criar uma matriz de compatibilidade: Windows, toolchain, hash do jogo,
  renderizador, configurações e resultado do teste com duas máquinas.

**Concluído quando:** falhas são diagnosticáveis e os cenários de regressão
relevantes podem ser repetidos a partir de instruções e dados próprios.

### M9 — Uso pelo jogador e releases

- [ ] Criar fluxo Rust para detectar/selecionar instalação, validar versão,
  configurar servidor e conectar/desconectar com mensagens claras.
- [ ] Verificar dependências do ambiente sem exigir o toolchain Rust do jogador.
- [ ] Empacotar somente os binários próprios, configuração de exemplo e avisos
  necessários; disponibilizar fonte e instruções de construção.
- [ ] Versionar protocolo e release; gerar hashes, changelog e lista de suporte.
- [ ] Testar instalação e remoção numa máquina sem a árvore de desenvolvimento.
- [ ] Publicar release somente após a entrega estar verificável e haver instrução
  do proprietário para publicar no destino escolhido.

**Concluído quando:** dois usuários com o jogo original conseguem instalar o
projeto e jogar o escopo LAN documentado sem assistência de desenvolvimento.

### M10 — Evoluções posteriores ao MVP LAN

- [ ] Relevância espacial e otimização para mais jogadores, conforme medições.
- [ ] Descoberta de salas LAN e UX de servidor dedicado.
- [ ] Internet: autenticação de sessão, proteção contra abuso, NAT/relay e modelo
  de confiança definidos antes de exposição pública.
- [ ] Persistência e gameplay compartilhado mais amplo, quando especificados.
- [ ] Compatibilidade com outras versões via novos perfis comprovados.
- [x] Validar ferramentas/servidor nativos em Linux e build cruzado Windows x86
  no Fedora; a integração inicial do jogo/DLL sob Proton também passou.
- [ ] Ampliar a matriz Proton para outras configurações e sessões longas;
  não depende de portar o jogo original nem de substituir a DLL por uma
  biblioteca Linux. Coop LAN entre máquinas ainda precisa de teste real.

## 6. Ordem de execução recomendada e riscos

O caminho principal é **M0 → M1/M2 → M3/M4 → M5 → teste LAN do MVP**.
M6 amplia gameplay; M7 melhora o pipeline; M8 acompanha cada mudança de risco;
M9 depende de resultados estáveis. Isso expressa dependências técnicas, não uma
instrução para criar agentes paralelos.

Prioridade imediata: descobrir criação de entidade remota sobre a integração
x86 controlada e o perfil entregues. Investigar cedo esse ponto evita
polir um transporte de rede sem uma forma de representar os jogadores no motor.
Não é preciso concluir toda a economia ou sincronizar todo o trânsito para
testar dois caminhões visuais na LAN.

| Risco/questão | Como reduzir e verificar |
| --- | --- |
| Offset não serve em outra instalação | Comparar hash, arquitetura e estados; criar novo perfil quando necessário |
| Ponteiro inválido no carregamento | Identificar ciclo de vida, suspender captura, invalidar referências |
| Chamada do motor trava/corrompe estado | Confirmar ABI/thread/ownership, testar operação mínima e teardown |
| Física diverge entre clientes | Começar com remotos visuais; definir autoridade antes de colisões |
| Unidades e origem interpretadas errado | Medir deslocamentos, retornos, rampas e consistência após reload |
| Dados perdidos ao trocar de computador | Commit/push dos fontes e documentos; configurar e reconstruir o ambiente local |
| Assets originais entram no Git/release | Lista de permissão, revisão dos arquivos e auditoria de empacotamento |
| Promessa maior que o que foi testado | Definir critérios por marco e manter limitações explícitas |

Sem estimativa rígida: tempo de rede é diferente do tempo de descobrir e integrar
um motor proprietário. Ao fechar um marco, revisar o próximo com base no que foi
comprovado, sem marcar trabalho futuro como pronto.

## 7. Publicar este estado no GitHub

O projeto é versionado como **fontes, lockfile e documentação**, com a licença
Apache-2.0 escolhida pelo proprietário. O remoto é
[rodolfoaltenrath/18wos-across-america-mp](https://github.com/rodolfoaltenrath/18wos-across-america-mp).
O commit inicial de licença foi preservado; não é necessário reinicializar Git
nem cadastrar novamente o remoto. O proprietário autorizou o envio inicial sem
coautor. Para entregas seguintes, na branch de trabalho:

```powershell
git status
git add -- .gitignore LICENSE AGENTS.md ROADMAP.MD README.md Cargo.toml Cargo.lock asset_tools multiplayer_server client_dll
git diff --cached --name-only
git diff --cached --stat
```

Confira a lista: devem aparecer somente arquivos próprios permitidos. Então:

```powershell
git commit -m "Descreva a alteração implementada"
git push
```

Autentique com o método configurado para sua conta; não grave credenciais em
arquivos ou URLs versionadas. Confira `git status`, `git branch` e `git remote -v`
antes de enviar, preservando o histórico existente. Não adicionar trailers de
coautoria aos commits, conforme solicitado pelo proprietário.

`.gitignore` não remove arquivos já rastreados; não use `git add -f` para incluir
originais. Ele também ignora novos tipos de fontes/documentos por padrão: novas
pastas de configuração, CI ou documentação precisam ser liberadas deliberadamente.
Esta é uma regra de organização deste projeto, não um mecanismo que impede um
envio forçado feito manualmente.

## 8. Continuar em outro computador

### O que o clone traz e o que precisa ser recriado

| Vai pelo Git | Não vai pelo Git |
| --- | --- |
| Fontes Rust e testes próprios | Instalação Steam e seus arquivos |
| `Cargo.toml` e `Cargo.lock` | `target/`, executáveis, DLLs compiladas e caches |
| `README.md`, `ROADMAP.MD`, `AGENTS.md` | `.cargo/config.toml` com caminhos desta máquina |
| Hash/offsets e evidências resumidas nos documentos | Saves, dumps `.aamem`, disassembly e logs de investigação |
| Futuras configurações portáveis expressamente liberadas | PIDs, bases de módulos e endereços absolutos da sessão |

### Procedimento no segundo PC Windows (alternativa)

1. Faça commit e push do trabalho neste computador antes de sair. Arquivos
   somente locais ou sem commit não aparecem automaticamente em outro PC.
2. No segundo Windows, instale Git e Rust via rustup. Para testar integração,
   instale o jogo original pela Steam e deixe-a preparar os pré-requisitos.
   Nesta máquina foram instalados DirectX e .NET Framework 3.5 na primeira abertura;
   isso é um registro local, não uma dependência de código Rust comprovada.
3. Clone **numa pasta nova de desenvolvimento**, fora de `Program Files`, por
   exemplo `C:\dev\across-america-mp`. Não execute `git clone ... .` dentro da
   instalação Steam já preenchida. O código não precisa ficar junto dos assets.

```powershell
git clone https://github.com/rodolfoaltenrath/18wos-across-america-mp.git C:\dev\across-america-mp
Set-Location C:\dev\across-america-mp
git status
```

4. Use o toolchain fixado atual: `1.99.0-x86_64-pc-windows-gnu`. O registro
   anterior era 1.98.1; a migração 1.99.0 foi testada no Fedora/Proton, e uma
   execução no Windows nativo ainda deve ser repetida. Esses comandos não
   devem ser usados como toolchain host no Linux.
   Esses comandos selecionam o toolchain para o diretório do clone:

```powershell
rustup toolchain install 1.99.0-x86_64-pc-windows-gnu
rustup override set 1.99.0-x86_64-pc-windows-gnu
rustup target add i686-pc-windows-gnu
rustup component add rustfmt clippy
rustup show active-toolchain
```

Se esse toolchain não estiver disponível no ambiente de destino, investigue a
origem/diferença antes de escolher outro; registre uma migração testada, sem tratar
uma troca de compiler/linker como automaticamente equivalente. `Cargo.lock` fixa
dependências resolvidas, não instala o compilador ou linker.

5. Leia no README a seção **Configuração GNU com os componentes distribuídos
   pelo Rust**. Execute o preparo no novo clone para gerar caminhos locais de
   linker/dlltool. Não copie a `.cargo/config.toml` absoluta deste computador.
   O fluxo atual exige esse preparo manual; automatizá-lo em Rust está em M0.
6. Compile e valide:

```powershell
cargo build --workspace --locked
cargo build -p client_dll --target i686-pc-windows-gnu --release --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo test -p client_dll --target i686-pc-windows-gnu --locked
```

7. Inicie o servidor e dois `client_probe` conforme o README. Esse teste usa
   trajetórias simuladas e não precisa dos saves ou endereços do primeiro PC.
8. Para inspecionar o jogo, use o caminho real da instalação Steam do segundo
   computador. Por exemplo, `asset_tools scan 'D:\SteamLibrary\steamapps\common\18 Wheels of Steel Across America'`.
   Com a partida aberta, use `memory_tool processes` e `modules --pid` para obter
   PID/base atuais. Verifique o SHA-256 de `aa/lib/game.dll` antes de reutilizar
   o perfil. O README contém a cadeia e o comando completo de observação/envio.
9. Mesmo com hash igual, valide as leituras no segundo PC. Com hash diferente,
   trate como nova versão; não aplique automaticamente os offsets conhecidos.
   Não é necessário transportar capturas antigas para tentar a cadeia documentada.
10. Saves de teste são opcionais e ficam fora do Git. Se quiser continuar a
    mesma partida, transfira seus saves por um meio pessoal, com backup no destino;
    não presuma que estejam no clone ou sincronizados pela Steam. Alternativamente,
    crie uma nova partida e faça a validação documentada.

Para uma nova sessão de desenvolvimento, abra o clone e forneça uma instrução
como esta; os arquivos versionados carregam o contexto técnico necessário:

> Leia AGENTS.md, ROADMAP.MD e README.md. Continue a partir da integração da DLL
> x86 no jogo, sem repetir a busca inicial de coordenadas. Verifique o ambiente
> e o hash local. Estão autorizadas grandes alterações e trabalho contínuo até
> concluir um marco verificável. Pergunte apenas por dependências reais.

O histórico desta conversa, permissões locais da ferramenta e processos abertos
não são arquivos do repositório. Uma nova sessão precisa do acesso ao clone e ao
ambiente apropriado; não depende de recuperar toda a conversa antiga.

### Procedimento no segundo PC Linux (destino informado)

O Linux pode ser a máquina principal de desenvolvimento. Não instale nem tente
executar o toolchain host `x86_64-pc-windows-gnu` como compilador nativo Linux.
Use o toolchain do host Linux e adicione `i686-pc-windows-gnu` como **target**
quando for gerar a DLL Windows. Não copie a configuração `.cargo` desta máquina.

| Componente | Uso no Linux | Validação já realizada |
| --- | --- | --- |
| `asset_tools` | Build/execução nativos; fornecer caminho local dos assets | Fedora 44: build, cinco testes e varredura dos três SCS |
| `multiplayer_server` | Build/execução nativos; pode atender clientes Windows por UDP | Fedora 44: build e relay local; LAN entre máquinas pendente |
| Protocolo e lógica portável do cliente | Build/testes nativos, com testes Win32 excluídos por `cfg` | Fedora 44: testes portáveis passaram; integração Win32 não executada |
| `client_dll` no jogo | Gerar DLL PE x86 e executar no prefixo Proton da Steam | Carga, captura, movimento, menu/outra partida e descarga passaram nesta instalação |
| `memory_tool` e `client_probe` | Binários Windows via ambiente compatível ou PC Windows | Versões nativas Linux não implementam essas funções |

Após instalar Git, rustup e as ferramentas de compilação nativas da distribuição,
use, num terminal Linux x86_64:

```sh
git clone https://github.com/rodolfoaltenrath/18wos-across-america-mp.git "$HOME/dev/across-america-mp"
cd "$HOME/dev/across-america-mp"
rustup toolchain install 1.99.0
rustup override set 1.99.0
rustup component add rustfmt clippy
cargo build -p asset_tools -p multiplayer_server --locked
cargo test --workspace --all-targets --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p multiplayer_server --locked -- --bind 0.0.0.0:39000
```

Use a mesma ressalva sobre disponibilidade/versão do toolchain descrita no fluxo
Windows. Build, testes, formatação e Clippy passaram com Rust 1.99.0 do Fedora,
conforme os registros de continuidade e integração; o preparo local rustup/MinGW
está no fluxo principal do README. Sessão LAN entre máquinas continua pendente.
A distribuição determina os pacotes de compilador
e linker; não instalar pacotes de outra distribuição por suposição.

O servidor Linux pode se comunicar com o jogo no PC Windows: configure o cliente
com o IP LAN do Linux, libere a porta UDP pertinente no ambiente e use o mesmo
protocolo. `127.0.0.1` só atende processos no mesmo computador. Não é preciso
executar o jogo no Linux para desenvolver assets, servidor e protocolo.

Para gerar a DLL, instale na distribuição um toolchain MinGW-w64 compatível com
alvo i686, incluindo GCC, binutils e dlltool. Depois, com essas ferramentas no PATH:

```sh
rustup target add i686-pc-windows-gnu
CARGO_TARGET_I686_PC_WINDOWS_GNU_LINKER=i686-w64-mingw32-gcc \
  cargo build -p client_dll --target i686-pc-windows-gnu --release --locked
```

A saída é `target/i686-pc-windows-gnu/release/client_dll.dll`; sem `--target`, um
build nativo Linux produz uma `.so`, que não substitui essa DLL. As versões dos
componentes MinGW precisam ser compatíveis com o target Rust. Instalar somente
o target com rustup não fornece necessariamente todo o linker/toolchain cruzado.
Execute testes Windows num Windows ou com um runner compatível configurado;
`cargo test --target i686-pc-windows-gnu` não executa `.exe` nativamente no Linux.

Para testar o jogo no próprio Linux, a opção a investigar é a instalação Steam
original com Proton, que fornece compatibilidade para jogos Windows. Isso não
garante suporte a outras instalações/configurações. A integração inicial nesta
instalação passou conforme o registro acima. Ampliar a matriz de arquitetura,
dependências, versão do Proton, prefixo,
enumeração de processos/módulos, carregamento da DLL e limpeza de hooks/threads.
Manter ferramentas Windows e jogo no ambiente compatível apropriado; não presumir
que um Wine separado vê o processo lançado pelo Proton. Registrar o hash local
e resolver a cadeia novamente, mesmo se os offsets conhecidos forem reutilizados.

Como alternativa imediata, desenvolver/compilar no Linux e testar a DLL no PC
Windows onde a leitura foi validada. Não é necessário reescrever o projeto nem
abandonar Rust. O suporte Proton só deve ser anunciado depois de passar nos
mesmos cenários de execução do Windows.

Referências: [targets windows-gnu e compilação cruzada](https://doc.rust-lang.org/rustc/platform-support/windows-gnu.html),
[suporte de plataformas Rust](https://doc.rust-lang.org/rustc/platform-support.html)
e [Proton na documentação Valve](https://partner.steamgames.com/doc/steamhardware/proton).

### Alternar entre os dois computadores

Antes de iniciar trabalho no computador de destino, com a árvore limpa:

```powershell
git status
git pull --ff-only
```

Antes de trocar novamente, revise os arquivos, adicione as alterações próprias,
faça commit e push. Pode usar uma branch como `feature/game-client` para uma
integração maior e continuar essa mesma branch no segundo PC. Se houver trabalho
local pendente ou o pull não puder ser fast-forward, preserve as alterações e
resolva a divergência; não use reset destrutivo ou force-push como rotina.
Recompile no destino; binários não acompanham esse fluxo de fontes.

Referências oficiais: [publicar código local no GitHub](https://docs.github.com/en/migrations/importing-source-code/using-the-command-line-to-import-source-code/adding-locally-hosted-code-to-github),
[clonar um repositório](https://docs.github.com/en/repositories/creating-and-managing-repositories/cloning-a-repository)
e [seleção de toolchain com rustup](https://rust-lang.github.io/rustup/overrides.html).

## 9. Registro de passagem para a próxima etapa

**Próxima entrega:** identificar e validar um caminho de modelo/render sem HUD,
controle local ou colisão; comprovar inicialização e inserção/remoção no mundo.
Lifetime privado independente, ownership do `c_item_t` e metadados estão validados.
O processo do último teste PMD foi encerrado pelo proprietário após restaurar a
sonda e parar a DLL; conferir novamente processos antes de reconstruí-la. Ampliar
a matriz de estados de M1 e a robustez de M2, sem refazer a busca ampla inicial.

**Antes de começar:** ler os três documentos, conferir Git e toolchain, localizar
a instalação local e conferir hash. Não refazer a busca ampla de memória se a
cadeia documentada funcionar na versão compatível. Não usar PIDs históricos.

**Ao terminar um bloco:** atualizar checkboxes, arquivos alterados, comando/cenário
de validação, resultado, limitações, decisão de arquitetura e próxima dependência.
O remoto e a licença Apache-2.0 já foram definidos pelo proprietário. A publicação
inicial dos fontes foi autorizada sem coautor; releases de binários ainda não
foram solicitadas. Preservar esse contexto ao continuar em outra máquina.
