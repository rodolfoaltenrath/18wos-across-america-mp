# Roadmap — Across America multiplayer em Rust

Última atualização: 28/10/2026. Este arquivo descreve o projeto, não uma promessa
de prazo. Checkboxes concluídos exigem implementação ou evidência registrada.
O objetivo é desenvolver ferramentas de assets e multiplayer para a instalação
original Steam de **18 Wheels of Steel: Across America**, com todo o código e
automação próprios em **Rust**. O jogo original permanece instalado separadamente
e não será distribuído pelo projeto.

**Ambiente atual: Windows, após retorno do Fedora em 28/10/2026.** Preservar o
fluxo Linux documentado e validar o host Windows com Rust 1.99.0. Ferramentas e
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
| Servidor | Tokio UDP, retransmissão com alvo de 60 Hz, rotação opcional, IDs, timeout, limites, validação e eco de teste | Sessão de protocolo explícita, identidade persistente, sincronização de relógio, controle de tráfego e mundo compartilhado |
| DLL | x86 no jogo sob Proton, perfil por hash, configuração ABI, worker, UDP, dispatcher e sonda do loop | Matriz completa de estados do jogo e APIs de entidades |
| Host de teste | `client_probe`, controlador entre processos e `test_host`; bancada x86 passou sob Proton | Testes de sessão longa, saída abrupta e duas máquinas |
| Investigação | `memory_tool` Rust: processos, regiões, busca, filtros, comparação, ponteiros, observação e envio externo | Perfis de versões e ciclo de vida do veículo |
| Coordenadas | Cadeia validada após movimento, reinício e comparação com save; pose de draw no mesmo corpo, escala em centímetros identificada | Outros estados/versões e amostragem consistente |
| Multiplayer visual | Eco com pose/rotação e textura iluminada vistos no jogo; liberação antes do load/recriação explícita passaram | Automatizar transição prévia ao unload, testar vários remotos e dois PCs na LAN |
| GitHub | Remoto `rodolfoaltenrath/18wos-across-america-mp`, histórico inicial Apache-2.0 preservado, exclusão do jogo | Manter staging auditado e documentação atualizada em cada entrega |

O estado atual **não é multiplayer LAN validado entre dois computadores**.
O proprietário confirmou um visual sem textura ao lado do seu caminhão,
copiando movimento e rotação; isso não foi um segundo jogador conectado.
As implementações posteriores têm testes próprios (último registro:
41 x86/47 nativos, Clippy e formatação). Eco/skin já passaram no reteste abaixo;
recarga automática e suavização na LAN seguem pendentes. Não rebaixar esse visual a mera
alocação privada, nem promover resultados da bancada a validação no jogo.

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

### Retomada no Windows nativo — 28/10/2026

O checkout retornou em `main`, igual a `origin/main`, no commit `9787f8a`.
Os avanços do Fedora foram preservados. O toolchain fixado 1.99.0 GNU x64
foi instalado no host Windows, com `llvm-tools-preview` e `rust-mingw` x86.
A configuração Cargo particular ainda apontava para 1.98.1; não foi copiada
para os fontes nem sobrescrita. Seu backup fica somente em `target/toolchain/`.

`dev_env windows` agora detecta o host e seleciona as ferramentas da versão
ativa: GCC/GNU ld tanto x64 quanto x86, LLVM dlltool em cache por toolchain,
PATH e `-B` para localizar o linker. O linker host também é passado pelo
ambiente para build scripts/macros; isso evita reaproveitar o GCC antigo
nessas etapas. `--native` seleciona o workspace Windows x64. O caminho Fedora
MinGW/rustup local foi preservado, mas não foi reexecutado neste Windows.
O helper não modifica persistentemente `.cargo/config.toml`.

Foi detectada uma diferença concreta entre linkers: com `rust-lld`, o teste
x86 que provoca e recupera um panic encerrava com código 3. Trocando somente
o fluxo de linkedição para GCC/GNU ld 1.99, o teste isolado e a bancada x86
completa passaram. A causa interna do unwind não foi isolada; não tratar um
build LLD bem-sucedido como equivalente a esta execução validada.

**Verificações concluídas:** build release x86 e workspace x64; 28 testes
nativos e 19 testes x86 executados pelos respectivos `test_host`; Clippy com
`-D warnings` em ambos os alvos. A bancada cobre a DLL release real, controle
entre processos, cancelamento/reentrância, lifetime privado, ownership de
itens, ABI stdcall e recuperação de panic. Os avisos GNU de aliases stdcall
continuam presentes no build release; a bancada valida os exports utilizados.

**Partida Windows:** `prism3d.exe` PID 12148, thread da janela 8104. Os três
hashes do perfil conferiram. A leitura externa recuperou a cadeia física e
13 propriedades PMD. A DLL foi carregada, `probe` executou na thread correta,
`registry` retornou 80 tipos e `inspect decals.pmd` confirmou 2700 bytes e os
callbacks perfilados. Consultas expiraram com o jogo fora de primeiro plano;
após o proprietário ativar a partida, passaram. O dispatcher foi removido,
sem trabalho pendente/em execução e sem instalar a sonda `pump-attach`.

No relay local Windows x64, a DLL do jogo enviou 965 pacotes e recebeu 871,
com zero falhas de leitura. Outro processo x86 com a DLL recebeu coordenadas
reais do jogo mudando durante o teste. `stop` e `unload` terminaram normalmente.
Esse teste é loopback no mesmo computador, não coop LAN nem prova de render.
Logs e artefatos ficam ignorados em `target/analysis/windows-return/`.

Uma segunda conexão acompanhou a sequência manual dirigir → menu de carregar
(aproximadamente 10 segundos) → carregar uma partida existente. Durante a
recarga, a cadeia ficou indisponível: `sample=2`, envio parado em 5005 pacotes,
105 falhas transitórias acumuladas. A captura retornou a `sample=1` sem reiniciar
o worker, e o envio retomou. A leitura externa confirmou novos endereços finais
(X em `0x0f3f81d0`, antes `0x12fcebf8` nesta sessão; Y/Z consecutivos). Outro
probe recebeu as novas coordenadas pelo relay. No encerramento, 7533 pacotes enviados, 483
recebidos, as mesmas 105 falhas, `stop`/`unload` concluídos e módulo ausente da
lista do processo. O relay e os probes próprios foram encerrados; o jogo ficou
aberto. Essa é uma linha validada da matriz, não a matriz completa: primeira
partida sem veículo, mudanças de mapa/caminhão, recargas repetidas, sessão longa
e saída abrupta permanecem pendentes. Não inferir que toda abertura de menu
invalida a cadeia ou bloqueia pacotes: a indisponibilidade observada foi na carga.

### M4 — Contratos estáticos candidatos para modelo PMD — 28/10/2026

A investigação offline confirmou o export de `p3shared.dll+0x1b10`,
`?me_string_set@@YA_NPAXPAUevalue_def_t@@IPBD@Z`: setter cdecl com objeto,
definição, índice `u32` e string C, retornando bool em AL. A implementação
usa `element_size`, `value_offset` e índice; a flag `0x02` seleciona
armazenamento indireto. Copia até `element_size - 1`, termina com NUL e, se
`auxiliary_offset >= 0`, aplica OR 6 no byte auxiliar. Não há validação de
índice/tamanho na rotina observada; o chamador precisa validá-los previamente.

Na partida nativa, `model_name` tinha flags 0, elemento 64, `table_bound=-1`,
offset `0x200`, auxiliar/contagem `-1` e setter `p3shared.dll+0x1b10`.
Isso delimita uma futura prova de string escalar inline, índice 0, máximo
63 bytes mais NUL, somente no PMD privado e exclusivamente pertencente à DLL.
Ainda não existe comando de escrita nem chamada desse setter neste bloco.

O construtor real PMD é `game.dll+0x107a90`; o wrapper perfilado permanece
`+0xf9c70`. O init candidato `+0x107d00` chama a base `+0xf2f10` e, na fase 0,
passa `model_name` e as skins pelo helper `+0x107c90` ao slot `+0x160e20`,
guardando o retorno em `objeto+0xa88`. O teardown candidato `+0x107c10`, fase 0,
solta esse recurso pelo slot `+0x16114c`, limpa outros recursos e chama a base
`+0xf2f40`. São evidências estáticas, não contratos completos de inicialização.

**Próximo bloco M4:** validar em fixture própria a ABI cdecl/string e os limites,
depois adicionar uma operação privada e restrita para `model_name`, pela fila
do loop, com leitura de confirmação e destruição equilibrada. Antes de init,
resolver os slots de recursos e efeitos da base, os pares init/teardown e
seu ownership. Não chamar init nem manter um PMD entre frames apenas com esta
análise. Não é necessário refazer os ciclos vazios já comprovados no Fedora.

### M4 — Setter privado de `model_name` — 28/10/2026

Entregue `engine model-cycle --model models/ai/truck/models/truck.psm
[--cycles 1..16]`. O comando usa somente `decals.pmd` e o item privado já
perfilado, na fila externa do loop. Confere a definição escalar, resolve e
compara o export `me_string_set`, relê a definição imediatamente antes da
chamada cdecl e escreve no índice zero. O objeto precisa ter `model_name`
inicialmente vazio; são conferidos o caminho com NUL e todos os bytes fora do
campo `+0x200..+0x23f`. O item destrói o objeto na mesma operação. Referências,
pool, bytes alocados/liberados e posição local continuam sendo verificados.
Não há recurso carregado ou objeto mantido entre frames.

O caminho é ASCII relativo `models/*.psm`, até 63 bytes, sem componentes
vazios, `.`/`..`, barras invertidas ou NUL. O contrato exige tipo **21**,
flags 0, elemento 64, offset `0x200`, limite/auxiliar/contagem `-1` e callback
`p3shared.dll+0x1b10`. O layout e versão das ABIs não mudaram: comandos 17/18
na extensão v3, `name` contém o caminho; o 18 é restrito ao host próprio.

**Bancada:** 28 testes nativos e 21 x86, build release, Clippy nativo/x86 e
formatação passaram. A fixture Rust própria tem PMD-sized storage com guardas
e um setter cdecl que devolve bool em AL deixando EAX alto preenchido. Foram
18 ciclos bem-sucedidos (16 com string de exatamente 63 bytes), três falhas
após construção (false, nenhuma escrita e efeito fora do campo), oito definições
incompatíveis, caminhos inválidos, modo errado e pedido cancelado durante loop
aninhado. Os 21 itens criados foram liberados, 56700 bytes equilibrados, todas
as referências restauradas e nenhum setter executado pelo pedido cancelado.
Nenhum asset comercial participa dos testes automatizados.

Na primeira tentativa real, a validação recusou o tipo genérico 14 usado
indevidamente na fixture, antes de alocar o item. O jogo informou tipo 21,
também confirmado na captura nativa anterior: `play_animation` usa 14 e
`model_skins` usa 25, embora compartilhem o descritor/callback de strings.
A sonda foi removida, o proprietário encerrou o processo e o contrato/fixture
foram corrigidos; tipo 14 passou a ser um caso explicitamente recusado.

**Jogo Windows, após reconstrução:** PID 13108, thread 19512, hashes do perfil
conferidos pela preparação. Passaram um ciclo e depois 16, com
`1826 / 1827 / 1826` referências, 2700 bytes por item, confirmação do nome e
nenhum byte externo ao campo alterado. A checagem da posição local passou
dentro de cada comando. Após o proprietário dirigir, fazer uma curva e
recarregar um save existente, mais um ciclo passou com `1846 / 1847 / 1846`.
Foram **18 ciclos reais de setter**, além dos ciclos vazios anteriormente
validados. O proprietário informou que controles, câmera e interface estavam
normais. `pump-remove` restaurou a entrada, `stop` removeu o dispatcher e as
filas ficaram sem pendências ou execução, quatro operações de fase concluídas
(probe e três comandos de ciclo). A remoção informou 169491 hits. O processo
foi encerrado normalmente e sua ausência foi verificada antes de nova edição
de binários. Os três originais do perfil e os dois saves mantiveram seus hashes;
as cópias de segurança dos saves ficam ignoradas em `target/backups/`.

**Dependência concreta para init/render:** os slots usados por
`game.dll+0x107c90` e pelo teardown PMD, em `+0x160e20` e `+0x16114c`, estavam
**nulos** na partida carregada, inclusive após a recarga. A instrução carregada
em `+0x107cea` confirmou o endereço real do primeiro slot após relocação.
Não foi feita nenhuma chamada a esses ponteiros. Não é seguro executar a fase 0
do init apenas porque o lifetime e a string passaram; descobrir quais etapas
ou módulos preenchem esses bindings, ou selecionar outro caminho de visual.
O init da base `+0xf2f10` também chama `+0xf3fe0(-1)`, que incrementa um contador
global em `+0x166598` e pode criar recursos compartilhados; foi observado valor
4. A chamada de teardown da base passa por `+0xf40b0`. Esse estado compartilhado
precisa de um par comprovado; não copiar o init completo de outro ator.

**Próximo bloco:** investigar registro/resolução dos callbacks PMD e o par de
recursos compartilhados da base. A escolha de `decals.pmd` continua candidata;
a ausência dos callbacks na partida impede habilitar init com o perfil atual.
Depois de comprovar os pré-requisitos, validar init/teardown em fixture e no
processo antes de inserir ou manter um visual entre frames. Não repetir os
ciclos de alocação/setter já comprovados sem uma mudança que justifique isso.

### M4 — Resolução estática e auditoria externa de recursos — 28/10/2026

Entregues dois comandos offline em Rust e uma auditoria externa somente de
leitura, sem novas chamadas de motor ou alteração da ABI da DLL:

- `engine_report MODULE pointers --rva RVA`: ocorrências sem alinhamento dos
  quatro bytes do VA preferido, separando seção executável/dados. Retorna o
  início dos bytes e falha se exceder o limite; não presume fronteira de código.
- `engine_report MODULE bindings --rva RVA --length N [--contains TEXTO]`:
  candidatos do padrão linear `GetProcAddress`/EAX/slot absoluto gravável.
  Aceita stores atrasados e a restauração do resolver antes do último store;
  interrompe em retorno, branch, chamada desconhecida ou operação não suportada.
- `memory_tool resources --pid PID`: confere quatro hashes quando o provedor
  está carregado, compara slots e exports nomeados do alvo, relê os slots e
  confere a identidade dos módulos ao terminar. Não injeta nem instala sonda.

**Evidência estática:** `DLL_register` de `game.dll` consulta o literal
`lib/psm` em `+0x15651c` e encaminha o resultado para a rotina `+0xce970`.
Nela, o decoder recuperou **87 associações** nome/slot. Seis foram incluídas no
perfil de auditoria, cruzadas com os exports do `lib/psm.dll` original:

| Export PSM | Slot em game.dll | RVA no provedor |
| --- | --- | --- |
| `psm_alloc_info` | `+0x16111c` | `+0x20e0` |
| `psm_info_release` | `+0x160ec8` | `+0x2650` |
| `psm_alloc` | `+0x160b4c` | `+0x3160` |
| `psm_alloc_simple` | `+0x160998` | `+0x31a0` |
| `psm_free` | `+0x160d80` | `+0x3750` |
| `psm_draw` | `+0x160ba4` | `+0x6a10` |

SHA-256 perfilado de `lib/psm.dll`:
`707ff239d5da3975c91b32fc6973e5d11627f18cfe79e1e3021b2ea56357d9b6`.
Para cada slot PMD anterior (`+0x160e20`, `+0x16114c`), a varredura de bytes
achou somente um operando de chamada no arquivo. Eles não aparecem entre os
87 bindings desse resolver. Isso reforça a investigação do caminho PSM usado
pelo jogo; não prova ausência de toda possível escrita indireta ou módulo
opcional. Não preencher os slots PMD com funções PSM por semelhança de nomes.

A auditoria distingue export correspondente, slot nulo, alvo inesperado,
ponteiro sem perfil e provedor ausente. Ponteiro em outro export do próprio
PSM também é inesperado. Um PMD não nulo continua sem ABI definida e não ganha
permissão de chamada. `psm_exports_match` significa apenas que as seis
comparações passaram; nenhuma prontidão de render é inferida.

**Validação automatizada:** 34 testes nativos e 24 x86 passaram, incluindo a
bancada da DLL release e as provas anteriores. Build release x86, Clippy
nativo/x86 e formatação passaram. Fixtures próprias cobrem operandos/dados
sem alinhamento, resultado atrasado, término da rotina, clobber de EAX/resolver,
branch, chamada estranha, slot inválido, rebasing, callback trocado, provedor
ausente, export alterado, mudança dos slots durante captura e memória ilegível.
Os relatórios comerciais completos ficam somente em
`target/analysis/resource-bindings/`.

**Contrato candidato seguinte, ainda sem chamada:** `psm_alloc_info` lê um
caminho por cdecl e retorna um descritor com referência inicial 1. O construtor
em `psm.dll+0x2070` chama `+0x1000`, que executa `vprocess_t::lock` sobre o
contexto em `+0xeb84`. `psm_info_release` decrementa a referência e, ao zerar,
passa por `+0x2090`: solta recursos do descritor e chama `+0x1010`, pareado a
`vprocess_t::unlock`, antes de liberar seu armazenamento. Essa análise delimita
uma futura prova do par, sem init de `decals.pmd`, HUD ou inserção no mundo.
ABI, referência do item carregado, efeito de cache e thread precisam de fixture
e validação no processo antes de habilitar a operação.

**Conferência real:** partida Windows PID 9888, aberta/carregada pelo
proprietário. A auditoria externa x64 e a release x86 produziram os mesmos
oito registros: seis `matched_export`, `psm_exports_match=true` e dois PMD
`unbound`, `pmd_slots_unbound=true`. Nenhuma DLL do projeto foi carregada no
alvo por essa auditoria. Não houve prova adicional de alocação, init ou render
neste bloco. O próximo passo é uma operação privada de info/release, com
fixture e ownership comprovados antes da chamada. Não refazer o setter nem
os ciclos vazios anteriores.

### M4 — Descritor PSM privado: carga e liberação — 28/10/2026

Entregue `engine info-cycle --model models/ai/truck/models/truck.psm
[--cycles 1..16]`. O comando usa somente `psm_alloc_info`/`psm_info_release`,
sempre na fila do loop externo, e libera cada descritor na mesma operação.
Não chama `psm_alloc`, `psm_draw`, init de PMD ou APIs de inserção no mundo.
A ABI continua v3/664 bytes, comandos 19/20; configuração/rede v1 preservada.
Callbacks arbitrários permanecem restritos à bancada própria, sem opção na CLI.

**Contrato delimitado e verificado:**

- `psm.dll+0x20e0`: cdecl com um caminho C; o caminho binário `.psm` resulta em
  descritor de 16 bytes, referência inicial 1, item de modelo em `+4`, array e
  contagem de animações nulos em `+8/+12`. `.psl` não é aceito pela ferramenta.
- `psm.dll+0x2650`: cdecl com um descritor; decrementa sua referência e, ao
  zerar, libera recursos, executa unlock e devolve o armazenamento ao provedor.
  Uma guarda Rust pareia essa chamada em todas as saídas de erro posteriores
  à alocação. O alocador Rust nunca recebe memória comercial para desalocar.
- Cabeça do contexto em `psm.dll+0xeb84`, contador direto em `contexto+8` e pai
  em `+12`; ponteiro/pai devem permanecer iguais dentro de cada operação.
- Referência forte do modelo em `c_item_t+0x4c`, confirmada pela implementação
  de `ci_dec_refs` em `prism3d.exe+0x13520`, e não em `+0x50`. Depois de soltar
  o descritor, há somente releitura pela API do sistema para verificar o
  decremento; não há dereferência Rust nem nova liberação do item. Se o pool
  resetar/devolver memória ilegível, o resultado é falha/inconclusivo.
- Além dos quatro hashes e seis exports PSM, conferir slots internos do PSM:
  `+0xe9d4` → núcleo `+0x13520` (`ci_dec_refs`), `+0xeb00` → `+0x13060`
  (`mm_precache`), `+0xea58` → `+0x13630` (`mem_sys_alloc`), `+0xeb28` →
  `+0x13700` (`mem_sys_free`). Callback executável e perfil exato são exigidos.

**Bancada:** provedor cdecl escrito em Rust, isolado dos assets comerciais,
com descritor, item e contexto próprios. A prova completa criou e liberou
23 descritores: 4 normais, 16 incluindo retenção inicial simulada, 2 para
detectar retenção repetida e 1 de recuperação. Cobriu retorno nulo, caminho
inválido, callback fora do módulo, modo jogo/fixture incompatível e tarefa
cancelada dentro de pump aninhado, sem execução tardia. Referência do modelo
voltou a 3, contexto ficou em 7 + 5 referências simuladas explicitamente
retidas e contador de criações igual ao de liberações. Os contextos dessa
fixture não constituem uma simulação completa do gerenciador de recursos.

**Verificações gerais:** build release x86, 25 testes x86 (incluindo DLL release
real na bancada), 34 testes nativos, Clippy x86/nativo e formatação passaram.
O aviso conhecido de resolução dos exports stdcall no GNU ld permanece; a ABI
foi exercitada pela bancada. Nenhum asset comercial entrou nos testes.

**Partida real:** mesmo processo Windows 9888, thread 11320; bindings rechecados
antes/depois e os quatro hashes do perfil intactos. Foram concluídos 18 ciclos:

| Cenário | Ciclos | Contexto antes/criado/depois | Saldo retido |
| --- | --- | --- | --- |
| Primeira carga do caminho escolhido | 1 | 922 / 924 / 923 | +1 |
| Repetições sem mudar de partida | 16 | 923 / 924 / 923 | 0 |
| Após dirigir, curva, menu e carregar save existente | 1 | 903 / 904 / 903 | 0 |

Todas as liberações balancearam a referência forte observada do item; a cadeia
de posição do jogador permaneceu igual dentro de cada operação. A primeira
carga deixou uma referência adicional no contexto: o recurso do modelo foi
acessado, mas não foi comprovada a liberação de todo cache/heap nem de toda a
árvore de módulos. O verificador aceita o primeiro saldo e recusa crescimento
repetido dentro de uma chamada, após liberar o descritor. Não interpretar a
queda de 923 para 903 entre cenários como responsabilidade desta operação;
houve recarga do mundo entre as capturas. Logs somente locais em
`target/analysis/resource-info/`.

As duas partidas de teste foram copiadas para backup ignorado antes de carregar
a DLL. Ao terminar, seus hashes e os quatro originais continuaram iguais.
A sonda foi restaurada (`pump_state=2`), dispatcher removido (`active=0`), sem
pendências/inflight e com 4 tarefas de fase completas (sonda + três comandos).
A DLL fica fixada até o processo terminar; o proprietário confirmou o fechamento
normal e a enumeração posterior não encontrou `prism3d.exe`. Nenhum descritor
fica vivo entre frames. Reenumerar processos antes do próximo build/ensaio.

**Próxima dependência:** investigar e provar o par de instância PSM/teardown
sem desenhar, com skins, capacidade, item e contexto contabilizados. As RVAs
`psm_alloc +0x3160`, `psm_alloc_simple +0x31a0` e `psm_free +0x3750` são pontos
de investigação; nomes/exports não autorizam reutilizar assinaturas do PMD.
A leitura estática mostra quatro argumentos de pilha no primeiro, três no
wrapper simple (que acrescenta 1) e um na liberação. A rotina `+0x31c0`
incrementa a referência do info e obtém outro item pelo callback `+0xeb40`;
`psm_free` acessa `+8/+0x50/+0x58` desse item e termina em `ci_dec_refs`.
Isso exige comprovar item versus bloco de instância, residência/acesso e
callbacks antes de qualquer chamada; não tratar seu retorno como o descritor
de 16 bytes nem como um ponteiro para memória a liberar diretamente.
Depois comprovar estado de render e transform, inserção/remoção e retenção
entre frames. Não inicializar `actors.car` com HUD nem preencher os slots PMD
nulos para contornar essa dependência. Caminhões remotos continuam pendentes.

### M4 — Instância PSM privada: implementação, bancada e partida — 28/10/2026

Entregue `engine instance-cycle --model models/ai/truck/models/truck.psm
[--cycles 1..16]`, na fila do loop externo. O caminho do jogo é restrito a esse
modelo; capacidade 1, sem skins fornecidas e sem animações. `engine_info`
compartilha somente os verificadores/guarda já usados; a nova fachada tem
guarda própria para a instância. ABI v3/664 bytes, comandos 21/22, configuração
e rede v1 preservadas. As assinaturas cdecl são:

- `psm_alloc_inst +0x31c0`: `(info*, skins**, skin_count, capacity) -> c_item_t*`.
  O info permanece retido pelo chamador, passando de 1 para 2 durante a instância.
- `psm_free +0x3750`: `(c_item_t*) -> void`; recebe o item gerenciado, e não
  os dados de tamanho variável em `item+8`. Libera shaders/pacotes, a referência
  de info, residência/acesso e a referência forte do próprio item.

O wrapper `psm_alloc +0x3160` cria/solta seu próprio info; não é usado pela
ferramenta. Manter o info visível permite verificar sua referência e os contadores
do item do modelo enquanto uma instância é criada e removida.

O perfil exige os quatro hashes e os seis bindings existentes, mais
`game.dll+0x161088` → export `psm_alloc_inst`. Confere seis callbacks internos:
`PSM+0xeb40` → núcleo `+0x131b0` (item anônimo), `+0xe94c` → `+0x13420`
(`ci_alloc_mem_fix`), `+0xeaec` → `+0x133f0` (MLRU add), `+0xe9ac` →
`+0x13410` (MLRU move), `+0xe9e8` → `+0x2720` (packet alloc), `+0xea24` →
`+0x2750` (packet free). O import `+0xb038` deve apontar a
`p3shared.dll+0xd820`, setter thiscall do item de um pacote. A tabela
`psm.dll+0xe568` deve coincidir com os nove campos perfilados, inclusive o
callback de movimentação `+0x2eb0` e o unlock `+0x1010`. A ferramenta não chama
esses callbacks diretamente: as chamadas são responsabilidade do provedor PSM.

**Verificações da instância:** item anônimo/carregado, referência forte 1,
residência 1, acesso 0 e identidade das operações. Dados alinhados e limitados
a 16 MiB, cabeçalho mínimo, ligação ao info, capacidade 1, escala inicial 1 e
campos de transformação inicialmente zerados. Contagens limitadas a 4096
esqueletos e 256 partes; arrays de shaders/pacotes dentro do bloco. Cada pacote
deve ter referência 1, ligar ao item correto e declarar o índice da parte.
Shaders compartilhados são agregados para conferir todos os decrementos.
Após `psm_free`, o info volta a 1, os contadores do modelo voltam ao baseline,
o item mostra referência forte zero e o contexto não cresce nas repetições.
O último info libera um lock do contexto. Releituras de itens já liberados são
diagnósticas via API do sistema: memória ilegível ou reciclada falha a prova,
sem dereferência Rust e sem nova tentativa de liberar o item.

**Bancada própria:** 24 instâncias de 256 bytes criadas/liberadas, seis infos,
três pacotes por instância e dois shaders, um deles usado por duas partes.
Exercitados quatro ciclos normais, 16 incluindo saldo inicial simulado, retorno
nulo, cabeçalho inválido com limpeza, crescimento repetido recusado após
liberação, recuperação e timeout dentro de pump aninhado sem chamada tardia.
Modo indevido, callback inválido, caminho inválido e limite de ciclos falham
antes da alocação. Ao final: 6144 bytes próprios alocados/liberados, pool com
referência zero, shaders de volta a 3/5, modelo de volta a 3 e contexto em
7 + 4 referências simuladas explicitamente retidas. A fixture não reproduz
todo o gerenciador de recursos comercial.

Build release x86, Clippy x86/nativo, formatação, 26 testes x86 e 34 nativos
passaram. Os testes de integração carregam a DLL release real na bancada.

**Primeira tentativa real:** PID 14068/thread 17784. A conferência recusou a
operação antes de chamar os alocadores: o perfil havia confundido o slot
`+0xe94c` com `mm_alloc_mem +0x13380`. A leitura externa e a associação nome/slot
confirmaram `ci_alloc_mem_fix +0x13420`; o perfil foi corrigido, sem remover a
verificação. Sonda/dispatcher removidos, filas vazias, worker parado e processo
encerrado normalmente pelo proprietário antes de reconstruir a DLL. Esse
ensaio recusado não conta como criação/liberação validada no jogo.

**Validação real após correção:** novo PID 2044/thread 19340. A leitura externa
conferiu a tabela de operações inteira, o setter de pacote e o slot do export
antes de instalar a sonda. O ciclo passou na fila de fase e criou um item com
bloco de dados de **26.268 bytes**. Foram 18 ciclos bem-sucedidos:

| Cenário | Ciclos | Contexto antes/criado/depois, info retido | Saldo módulo info/instância |
| --- | --- | --- | --- |
| Primeira carga | 1 | 897 / 898 / 897 | +1 / 0 |
| Repetições na mesma partida | 16 | 897 / 898 / 897 | 0 / 0 |
| Após dirigir, curva, menu e carregar save existente | 1 | 892 / 893 / 892 | 0 / 0 |

As verificações de cabeçalho, referências dos shaders, item devolvido ao pool,
contadores do modelo e info `1→2→1` passaram. Após cada comando, o último info
também liberou um lock. A posição do caminhão local não mudou dentro de cada
operação. A diferença de baseline após recarga não é atribuída ao diagnóstico.
O saldo inicial +1 pertence ao caminho de info/modelo já observado; nenhuma
retenção adicional de módulo foi observada nos ciclos de instância. Isso não
mede todos os bytes ou recursos transitivos do gerenciador comercial.

Sonda restaurada (`pump_state=2`), dispatcher removido (`active=0`), worker
parado, filas/inflight zerados e quatro tarefas de fase completas. Backups dos
dois saves foram preparados antes da carga da DLL; seus hashes e os quatro
originais continuaram iguais. O proprietário confirmou o encerramento normal;
a enumeração posterior não encontrou o jogo. Logs/disassembly/cópias comerciais
somente em `target/analysis/resource-instance/`, fora do Git.

**Limites:** sem skins próprias, o PSM usa `/shaders//specials/notfound.shd`.
Não há draw, física, HUD, world insert ou instância persistente entre frames.
Não tratar a fila do loop como fase de render comprovada, nem declarar todo
heap/cache liberado pelos contadores diretos de módulo observados.

**Próxima dependência:** comprovar fase, estado de matrizes/câmera, skins e
lifetime da fila gráfica antes de desenhar. A análise estática de `psm_draw`
mostra `r_proto_t::set_vpacket` (p3shared `+0xd830` incrementa a referência do
pacote) e chamada a `gfx_queue_proto` pelo slot PSM `+0xe744` em `+0x6cf1`.
Depois, o draw decrementa a referência local do proto, que pode continuar vivo
na fila. Logo, o ciclo criar/desenhar/liberar imediatamente não é autorizado
pela prova atual: precisam ser identificados o consumo/drenagem e as referências
que ainda usam o item e seus dados. O shim de PeekMessage não comprova essa
fronteira de render. Investigar observação passiva antes de chamadas gráficas.
Depois validar um único visual persistente, transform, remoção em menus/reload
e integração de jogadores. Não repetir estes 18 ciclos sem mudança que exija.

### M4 — Observação passiva do render e das filas — 28/10/2026

Entregues `engine render-attach`, `render-arm`, `render-report` e
`render-remove`. A sonda observa somente chamadas já feitas pelo jogo, sem
solicitar draw nem criar recursos. Exige dispatcher e sonda de loop instalada,
que já fixa a DLL até o processo terminar. O controle usa ABI v3/664 bytes,
comandos 23–28; registros/matrizes usam campos com codificação própria em
`render_observation`, sem alterar layouts/comandos anteriores ou a rede v1.

**Contrato estático usado pela observação:**

- Núcleo `+0x22240`: import de
  `gfxshared.dll!?render_pool@r_device_t@@QAEXPAUr_pool_t@@@Z`, export `+0x3690`.
  ABI thiscall com `this` em ECX, um pool na pilha, retorno void e `ret 4`.
  O shim Rust naked captura o retorno real antes do prólogo, alinha a pilha e
  encaminha a chamada exatamente uma vez, preservando os registradores não
  voláteis pela ABI. O getter de `LastError` é salvo/restaurado antes do original
  e depois dele, inclusive quando leituras da observação falham.
- Jogo `+0x160ea0`: binding de `gfx_path_flush`, núcleo `+0x4820`, cdecl sem
  argumentos de pilha. Callers não consomem retorno. A rotina percorre os pools,
  chama render e depois `gfx_pool_remove_all +0x3b60`; no final chama `+0x1070`.
- O render do pool não esvazia a lista observada. `gfx_pool_remove_all` percorre
  buckets/protótipos, decrementa referências e chama `r_proto_t::free` quando
  necessário, depois limpa a lista interna via `vptrlist_t::remove_all`.
  A sonda não chama essas funções diretamente e não modifica seus dados.
- Matrizes lidas dos ponteiros do núcleo `+0x5e000` (world, getter `+0x3190`)
  e `+0x5e848` (camera, getter `+0x3560`), 64 bytes de cada, sem interpretação
  de convenção ou origem. Não chamar getters do motor a partir da sonda.

Além dos hashes já perfilados de núcleo/engine/jogo, acrescentado SHA-256
de `gfxshared.dll`:
`947d76cf3a905079ed7ac75b5dea35a2734582db35ad194aec8a130c606898ba`.
Exports e valores dos dois slots são conferidos antes de qualquer alteração.
Troca/restauração usam compare-exchange e preservam a proteção original de
memória, com rollback/estado retido em falha parcial. Outro proprietário de
slot impede sobrescrita. Os originais e dados estáticos permanecem válidos
após remover, pois outra thread pode ter carregado o shim antes da restauração.
`engine pump-remove`/`stop` removem primeiro a sonda gráfica; erro impede
prosseguir com a limpeza como se tivesse passado.

**Captura:** uma geração acompanha o próximo flush externo completo da thread
selecionada. Registra entrada, render enter/return, cada pool depois do flush e
retorno final. Limites: 128 registros, 16 pools distintos, 256 nós compartilhados
entre as duas listas por leitura. Truncamento, ciclo, leitura inválida, nesting
e perda por disputa do buffer ficam explícitos. Callbacks usam `try_lock`,
sem espera no mutex nem alocação por registro; falha durante uma captura a
invalida. A CLI consulta uma geração fixa e recusa mistura com outra captura.
O relatório completo deve ter `summary.flags=0`; campos `empty_observed` só
descrevem uma fila lida integralmente naquele ponto da thread observada.

**Bancada própria:** host Rust com slots em página somente de leitura, cdecl
flush e thiscall render, duas listas e dois protótipos próprios, além de world/
camera. Passagem de `this`, pool, pilha e `LastError` foram exercitados pela DLL
release real. Captura de cinco eventos: fila `2 → 2 → 0`, com matrizes copiadas.
Foram cobertos pré-requisito de pin, callback/modo inválidos sem instalação,
matriz ilegível sem quebrar o encaminhamento, chamada de outra thread sem
incluir seus dados, geração antiga/índice inválidos e restauração automática
no stop. Após remover, o host continuou renderizando; ao sair, conferiu ambos
os originais e `PAGE_READONLY`. O decoder próprio cobre listas vazias/vivas,
ciclos, ponteiro inválido, orçamento, bits exatos das matrizes e NaN.
O host também provocou 100 renders no mesmo flush: a captura parou de ler
novos registros no limite de 128 e marcou o relatório incompleto; uma nova
geração recuperou a captura normal. Ao simular outro proprietário do slot,
remoção/stop recusaram sobrescrevê-lo, sem descarregar a DLL; devolver o slot
à sonda permitiu concluir a limpeza e restaurar sua proteção.

**Verificações gerais:** build release x86, Clippy x86/nativo, formatação,
28 testes x86 e 35 nativos passaram. A bancada não usa arquivos comerciais.
Não houve chamada nova de render pela ferramenta.

**Validação nativa na partida:** processo histórico 9044, thread 17156. Antes
da instalação, os quatro hashes e os dois slots originais conferiram; saves
copiados para diretório ignorado. Foram capturados três flushes completos:

| Estado confirmado pelo proprietário | Geração | Registros | Pool A | Pool B |
| --- | ---: | ---: | --- | --- |
| Partida parada em primeiro plano | 2 | 8 | `9 → 9 → 0` | `1 → 1 → 0` |
| Após dirigir e fazer curva, mesma câmera | 3 | 8 | `9 → 9 → 0` | `1 → 1 → 0` |
| Após menu de carregar e recarga de save existente | 4 | 8 | `9 → 9 → 0` | `1 → 1 → 0` |

Cada sequência compara entrada/retorno do render e retorno do flush. Pools
`núcleo+0x29558` e `+0x29648`, um bucket em cada, listas integralmente legíveis,
sem truncamento. Os três relatórios tiveram `flags=0`, `foreign_calls=0` e todos
os eventos na thread 17156; world/camera legíveis e finitas nos 24 registros.
As matrizes mudaram após movimento e após recarga. O retorno do flush capturado
foi sempre `game.dll+0x800f2`; o retorno de render, núcleo `+0x28e1`.
Isso comprova a observação desse passe, não a cobertura de todos os passes 3D.
O menu foi atravessado pelo proprietário; não houve captura isolada do menu
nem de um instante durante o carregamento. A matriz completa de estados segue
pendente. Artefatos locais: `target/analysis/render-observation/`.

Na tentativa inicial, o loop continuou ativo sem chamadas de render/flush
enquanto a janela estava sem foco. Voltar à partida em primeiro plano concluiu
a geração já armada. Uma tentativa de armar a geração 3 durante flush foi
recusada; repetir entre flushes funcionou. Não rearmar só para consultar estado:
a CLI agora oferece `engine render-status`, JSON de contadores/flags sem ler
registros nem modificar a geração, inclusive enquanto a captura está pendente.
A bancada recebeu pausa/retomada do render para verificar esse caso pela CLI.
O teste libera o controlador antes de executar `mp_launcher`, pois o mutex de
controle por PID recusa um segundo controlador (comportamento intencional), e
reabre o controle depois; a geração pendente permanece inalterada. Após o ajuste,
28 testes x86 e 35 nativos passaram, com Clippy x86/nativo e formatação limpos.

**Limpeza real:** `render-remove` restaurou os dois slots, conferidos por leitura
externa: núcleo `+0x22240` voltou a `gfxshared+0x3690`; jogo `+0x160ea0` voltou
a núcleo `+0x4820`. `pump-remove`/`stop` deixaram dispatcher inativo, worker
parado, sonda do loop removida e contadores de trabalho pendente/em execução
zerados. Os quatro módulos e os dois saves conservaram seus hashes. O usuário
confirmou ausência de problemas e encerrou normalmente; enumeração posterior
não encontrou `prism3d.exe`. Só então a reconstrução foi iniciada.

**Limites e próxima dependência:** fila CPU vazia não é fence da GPU nem prova
de outras filas/passes. Os getters globais mantiveram os mesmos valores ao
longo de cada flush; não foram capturadas as matrizes de cada protótipo nem
comprovada sua equivalência ao transform do caminhão. Identificar o passe 3D
e o momento de submissão adequado, contexto de câmera/matrizes, skins e
ownership dos pacotes antes de introduzir um visual persistente. Não repetir
criação/liberação privada já comprovada sem mudança que exija a regressão.

### M4 — Passe 3D e ponto de submissão (estático + sonda) — 28/10/2026

**Ferramenta:** `engine_report disassemble --resolver RVA` (repetível) anota
operandos absolutos com nomes de bindings recuperados; `Instruction.slot` traz
o RVA absoluto decodificado. Teste próprio cobre IAT e stores atrasados.
Resolver do núcleo em `game.dll+0xcdce0`: **151 bindings** `gfx_*` (mapa local
em `target/analysis/game_core_bindings.*`, não versionado). Resolvers de
`psm.dll` em `+0x11d6/+0x12b6/+0x15f6/+0x16c6/+0x1e86/+0x1fc6`.

**Conclusões estáticas** (somente leitura de arquivos; endereços desta versão):

- O retorno `game.dll+0x800f2` está em `+0x7ffc0`, chamado por `+0x7fa90`,
  que sobrescreve `entity_t::draw` numa vtable (`+0x12618c`): `gfx_push_identity`,
  translação até a câmera, `gfx_cache_data`/`gfx_sprite` e flush. É um passe de
  céu/sol, não "o passe 3D" inteiro. O núcleo não chama `gfx_path_flush`
  internamente; todos os flushes vêm de `game.dll` (`+0x800ec`, `+0x8018b`,
  `+0x8e660`, `+0xccb43`, `+0xccd1c`, `+0xf3a24`). `begin/end_frame`/swap em
  `game.dll` aparecem só em telas de carregamento.
- Submissão: `psm_draw` é cdecl `(instância, bool cull, ARGB)` (`-1` = branco;
  interior usa `[this+0x304]|0xff000000`). Aloca protos, define pacotes/shader
  e chama `gfx_queue_proto` no path corrente, liberando a referência local.
  `gfx_queue_proto` (núcleo `+0x4740`) copia o topo das pilhas world
  (`+0x5e000`), camera (`+0x5e848`) e viewport (`+0x5ee2c`) para o proto e
  insere via ponteiro `path+0x88`; sem viewport/info registra `[r_path] NULL ...`.
  Path corrente `+0x5ea7c`, info `+0x5ea74` (`gfx_path_select` grava ambos).
- Pilha world: topo em `+0x5e000`, 16 entradas de `0x84` (matriz, inversa,
  cache) em `+0x5e004..+0x5e844`. `gfx_push` satura sem erro no limite, mas
  `gfx_pop` decrementa: um draw próprio precisa conferir profundidade antes.
  `gfx_translate(vec3*)`, `gfx_qrotate(quat*)` (pula w=1) e `gfx_mult(m, inv)`
  são cdecl e multiplicam o topo.
- Padrão do jogo em ~26 sítios: `gfx_push → gfx_translate → gfx_qrotate/rotate
  → psm_draw(inst,1,cor) → gfx_pop`.
- `actors.car`: vtable `game.dll+0x1257a0` (construtor `+0x50b40`);
  `pre_draw` `+0x68` → `+0x64e20`; `draw` `+0x6c` → `+0x5a6a0`. Com estado
  `+0x1e9 == 4` desenha o interior em espaço de câmera; caso contrário chama
  `+0x64ea0`: corpo `[[this+0x1d0]+0x3e4]`, quaternion `+0x14`, posição `+0x30`;
  `gfx_push → gfx_translate(pos) → gfx_mult(get_matrix, get_imatrix)` e partes
  (rodas `+0x30c/+0x310`, ossos via `psm_bone_get_matrix`).

Logo, o ponto candidato para um visual remoto é logo após o retorno do `draw`
original de `actors.car`, na mesma thread/pilha, antes do flush do frame. Isso
ainda é inferência estática: precisa da sonda abaixo na partida.

**Sonda passiva entregue:** `engine draw-attach/arm/status/report/remove`,
comandos 29–34, ABI v3/664 bytes preservada, módulo separado `engine_draw`
(a sonda de render validada não mudou, só expôs leitura/contador). Detalhes
de campos no README. `pump-remove`/`stop` removem a sonda de draw antes da de
render. Instalação uma vez por processo; slot conferido; compare-exchange e
proteção original restaurada; outro proprietário impede sobrescrita.

**Bancada própria:** fixture `--draw` com vtable em página somente leitura,
objeto com cadeia de corpo, pilha world com cabeçalho e profundidade 2, path/
info/viewport e flush da fixture de render após cada `draw`. Passaram:
pré-requisito de pump, original incorreto recusado sem instalar, instalação
dupla recusada, 32 registros alternados na thread esperada, `this`/vtable/
estado/path/viewport/profundidade, pose e matrizes coerentes por frame, mesmo
`flushes_seen` em entrada/retorno e +1 entre frames, chamada estrangeira só
contada, corpo ilegível sem quebrar o encaminhamento, geração antiga e índice
inválido recusados, `draw-arm/report/status` pela CLI, remoção restaurando o
original (chamadas continuam, contador da sonda para) e PAGE_READONLY no fim.
30 testes x86 e 37 nativos passaram; Clippy x86/nativo e formatação limpos.

**Validação nativa na partida (28/10/2026):** processo histórico 7828, thread
da janela 7892, `game.dll` em `0x4cb0000`. Hashes de núcleo/jogo/p3shared/
gfxshared/psm e dos dois saves conferidos antes e depois; saves copiados para
`target/analysis/draw-observation/` (não versionado). Pump, render e draw
instalados nessa ordem. Três capturas completas (`flags=0`, 32 registros,
`foreign_calls=0`):

| Estado confirmado pelo proprietário | `this` | Pose |
| --- | --- | --- |
| Partida parada | `0xc578b7c` | fixa, igual ao save |
| Após dirigir e fazer curva | `0xc578b7c` | `z` +~1/frame; quaternion mudou |
| Após menu e recarga do save | `0xd9b49a8` (novo) | voltou à pose do save |

- Um único `actors.car` desenhado (o jogador), sempre do laço de entidades
  `game.dll+0xf3782` (retorno `+0xf3785`), na thread da janela.
- O `draw` ocorre duas vezes por ciclo, alternando: estado `+0x1e9 = 6` com
  pilha world na profundidade 0, viewport `núcleo+0x5ea80`; e estado 12 com
  profundidade 1, viewport `+0x5eae8`. Pelo código estático, 12 retorna sem
  desenhar. Path corrente igual nas duas; ~4 flushes entre `draw`s.
- World = identidade na entrada (o `draw` aplica a posição absoluta). Pose
  igual em entrada/retorno; muda uma vez por frame, entre o estado 12 e o 6.
- Amostras da sonda de render na mesma sessão: o flush do céu (`+0x800f2`,
  filas 9/1) e, numa geração, o flush do mundo no mesmo laço de entidades
  (retorno `game.dll+0xf3a2a`, quatro filas com 144/9/30/61 protos). Logo, o
  `draw` de estado 6 enfileira no passe do mundo esvaziado em `+0xf3a24`.
- **A recarga recria a entidade do jogador**: nunca guardar o ponteiro do
  caminhão local entre carregamentos; localizar novamente pelo `draw`.

Limpeza: `draw-remove`, `render-remove`, `pump-remove`, `stop`. Leitura externa
mostrou `game+0x12580c → game+0x5a6a0`, `núcleo+0x22240 → gfxshared+0x3690`,
`game+0x160ea0 → núcleo+0x4820`; o contador de draws da sonda parou. A DLL
continua fixada até o jogo encerrar (regra da sonda do loop).

Não comprovado: o significado exato de 6/12 (espelho/sombra/outro passe), a
convenção da matriz de câmera, interior (estado 4) e caminhões de IA (outro
tipo ou ausentes nesta cena).

### M4 — Primeiro visual de teste: implementação e bancada — 28/10/2026

**Escala:** o `truck.psm` de IA (extraído localmente em `target/analysis/`,
não versionado) tem caixa de ~250 × 290 × 680 unidades, ou 2,5 × 2,9 × 6,8 m:
**a unidade do mundo é o centímetro**. A câmera estava a ~324 cm do corpo.

**Entregue:** `engine visual-show [--model M] [--offset X Y Z]`, `visual-hide`,
`visual-release`, `visual-status` (comandos 35–39, ABI v3/664 preservada).
Módulo `engine_visual`:

- Criação na fila do loop (`visual-show`): reaproveita os contratos validados
  de info/instância (`psm_alloc_info`, `psm_alloc_inst(info,NULL,0,1)` e
  `check_instance`), sem skins (shader de fallback). Restrito a
  `models/ai/truck/models/truck.psm` no jogo. Confere exports e slots do jogo de
  `psm_draw` (`psm+0x6a10`, slot `+0x160ba4`) e `gfx_push/pop/translate/qrotate`
  (núcleo `+0x2de0/+0x2eb0/+0x30e0/+0x3140`, slots `+0x16104c/+0x160a98/+0x160fd8/
  +0x160fe4`). Exige as sondas de draw e de render no mesmo modo.
- Desenho: dentro da sonda do `draw`, depois do original, na mesma thread.
  Somente se o estado não for 4/9/12 (o mesmo ramo externo do jogo), com a pilha
  world na profundidade 0, path/info/viewport não nulos e o corpo legível. Faz
  `gfx_push → gfx_translate(pos+offset) → gfx_qrotate(quat do jogador) →
  psm_draw(item,1,-1) → gfx_pop` e confere o topo da pilha depois; divergência
  marca `fault` e para de desenhar. Offset em eixos do mundo, padrão 400 cm em
  X, limite 100 m. `LastError` é preservado.
- Liberação na fila do loop (`visual-release`): exige o visual oculto, pelo
  menos 8 flushes desde o último desenho/ocultação e contadores do item de volta
  a `[1,1,0]`. O `+0x58` (acesso) é o que `psm_draw` incrementa e o consumo do
  passe devolve. Senão, recusa e mantém a instância.
- Guardas: `draw-remove` recusa com o visual exibido; `render-remove`,
  `pump-remove` e `stop` recusam com a instância alocada.

**Bancada própria:** a fixture de draw ganhou profundidade/estado selecionáveis
e `psm_draw`/`gfx_*` falsos sobre a mesma pilha. O desenho falso incrementa
`+0x58` e o "flush" o consome. Passaram: exigência das sondas e do modo,
offset fora do limite, nada desenhado na profundidade 2 nem no interior (só
`skipped`), desenho na profundidade 0 com pilha +1, pose/offset/quaternion
corretos, item e cor/cull esperados. Também passaram as guardas de remoção,
`release` recusado com o visual exibido e com acesso pendente após ≥8 flushes,
liberação depois do consumo (instâncias criadas = liberadas), recriação,
`visual-hide` pela CLI e liberação depois de assentar. 32 testes x86 e 38
nativos, com Clippy e formatação limpos.

**Validação nativa na partida (28/10/2026):** processo histórico 10056, thread
da janela 2360, sem DLL do projeto antes do teste. Hashes e saves conferidos
antes e depois (cópias em `target/analysis/visual-test/`, não versionadas).
Sequência: pump, render, draw, `visual-show --offset 400 0 0`.

- O proprietário viu **um segundo caminhão ao lado do seu**, sem textura (shader
  de fallback, como previsto), acompanhando posição e rotação do jogador.
- Com a janela fora de foco, o jogo não renderizou (flushes parados) e nada foi
  desenhado; ao voltar o foco, o desenho começou.
- Primeira exibição: 3268 desenhos, 3268 pulados (as chamadas de estado 12),
  `fault=0`, acesso do item `0` entre frames. Oculto e liberado; as primeiras
  ~21 tentativas de liberação (~10 s) foram recusadas, com mensagem não
  capturada pelo script (provável foco/fila do loop). Depois liberou com
  `state=0`.
- Recarga do save **depois** da liberação; `visual-show` recriou a instância e
  desenhou ao lado da nova entidade do jogador (o proprietário confirmou).
  8059 desenhos totais, sem falha. Ocultar + liberar passou na primeira
  tentativa, sem recusas registradas.
- Limpeza: `draw-remove`, `render-remove`, `pump-remove`, `stop`. Leitura
  externa: `game+0x12580c → game+0x5a6a0`, `núcleo+0x22240 → gfxshared+0x3690`,
  `game+0x160ea0 → núcleo+0x4820`. Hashes de originais e saves iguais.

Ainda não comprovado: skins/textura corretas, ajuste ao solo (o
proprietário não relatou desvio de altura), interior (estado 4), manter a
instância durante uma recarga, e mais de um visual. A pose ainda é a do
jogador mais um deslocamento: falta alimentá-la pela rede.

### M4/M3 — Pose pela rede para o visual de teste (bancada) — 28/10/2026

Entregue sem teste no jogo, a pedido do proprietário:

- **Protocolo:** `Rotation` opcional (quaternion) em `ClientUpdate` e
  `ServerUpdate`, com `serde(default)`. Pacotes v1 sem rotação continuam
  válidos e a versão segue 1. Quaternion não finito ou fora de
  `0.98 ≤ |q|² ≤ 1.02` é rejeitado; o receptor nunca normaliza em silêncio. O
  servidor retransmite a rotação.
- **Fonte local `DRAWN` (modo 4 da configuração v1, `connect --mode drawn`):**
  a cada `draw` do caminhão, a sonda publica posição e quaternion do corpo
  (`try_lock`, sem bloquear o render). O worker envia essa pose com rotação se
  ela tiver menos de 0,5 s; caso contrário, pausa e informa. Assim, posição
  enviada e desenho usam o mesmo sistema de coordenadas. A fonte `GAME` antiga
  (só posição, cadeia descoberta por varredura) foi mantida. A revisão posterior
  confirmou que ambas leem a mesma estrutura do corpo; ver a correção abaixo.
- **Alvo remoto do visual (`visual-show --player ID`):** usa a entrada mais
  recente do jogador em `REMOTES` (`try_lock` na thread de render), somada ao
  offset; sem rotação, usa identidade. Acima de 1 s não desenha (`skipped`).
  `RemoteState`/`client_get_remote` (ABI C) permanecem inalterados; a rotação
  fica numa entrada interna à parte.
- **Bancada:** testes do protocolo (com/sem rotação, quaternions inválidos) e do
  servidor (retransmite rotação, ausência preservada), teste unitário da fonte
  `Drawn` (pose fresca/velha) e o teste ponta a ponta `remote_visual_workflow`:
  servidor UDP falso recebe a pose da fixture com rotação; "jogador 5" faz a
  fixture desenhar exatamente em `(5000, 10+1, −20)` com o quaternion
  enviado; amostra mais nova move o visual e a mais antiga é ignorada; após 1 s
  sem dados para de desenhar; jogador desconhecido não desenha; liberação,
  `pump-remove` e `STOP` limpos. 34 testes x86 e 39 nativos, Clippy e
  formatação limpos.

Pendente no jogo: dois computadores na LAN, cada um com `connect --mode drawn`
ao servidor e `visual-show --player <id do outro>`. Antes disso, um teste
local: o mesmo PC com um par falso, ou espelhando o próprio jogador via servidor.
O ID do outro jogador é atribuído pelo servidor; `engine remotes` (abaixo)
passou a listá-lo.

**Correção:** `profile::POSITION` (fonte `game`) termina em
`…->0x1d0->0x3e4->0x30`, a mesma posição do corpo lida pela sonda de `draw`.
As duas fontes usam o mesmo sistema de coordenadas; `drawn` acrescenta a
rotação e só envia com um `draw` recente.

### M4/M5 — Blocos sem jogo: interpolação, sessão, skin, recarga — 28/10/2026

Entregues e validados somente na bancada própria; cada bloco teve commit
automático. `dev_env verify` terminou com 41 testes x86, 46 nativos, Clippy
x86/nativo e rustfmt limpos.

1. **Interpolação e vários remotos** (`interpolation.rs`). Histórico de 16
   amostras por jogador, com tempo local de recepção, desenhado 100 ms atrás:
   lerp de posição e nlerp de quaternion pelo arco curto. Extrapola no máximo
   200 ms e faz snap em teleporte (>20.000 cm/s). `visual-show --player all`
   desenha todos os remotos frescos com a **mesma** instância, até 16 por frame
   (o jogo desenha uma instância de roda várias vezes por frame). A pilha é
   conferida após cada desenho. `engine remotes` (comando 40) lista ID,
   sequência, idade, amostras, posição e rotação. A bancada provou valor estável
   após amostras repetidas, sequência antiga ignorada, parada após 1 s, jogador
   desconhecido sem desenho, razão de 2 desenhos por frame com dois jogadores e
   a listagem pela CLI.
2. **Sessão em um comando** (`launcher_session`).
   - `session-start --server A:P [--player all] [--offset ...] [--backup NOVA]
     [--saves DIR] [--no-skin]` executa load, attach, pump, render, draw,
     `connect --mode drawn` e `visual-show`, parando no primeiro passo recusado e
     relatando qual foi.
   - `--backup` copia os saves para uma pasta nova e grava SHA-256 dos cinco
     originais e dos saves.
   - `session-status` reúne cliente, visual, draw e remotos.
   - `session-stop [--timeout s] [--verify DIR]` oculta, repete a liberação
     guardando as mensagens de recusa, remove draw/render/pump, para o cliente e
     confere os três slots por leitura (perfil do jogo).
   - `session-verify --backup DIR` compara os arquivos sem chamar a DLL.

   Na bancada, a fixture passa seus descritores com `--fixture-info`. Foram
   cobertos a segunda partida recusada, status, parada com liberação e um save
   alterado sendo detectado com a cópia preservada.
3. **Skin.** Análise estática: o jogo cria modelos com `psm_alloc(path, NULL, 0,
   16)` e aplica `psm_set_shader(item, parte, caminho)` (`psm+0x3a60`, slot
   `+0x16102c`). Essa função faz `mm_precache` e `psm_set_shader_item`
   (`+0x39e0`), que libera o shader anterior da parte com `ci_dec_refs` e
   **consome** a nova referência. O `car.def` de IA declara um único
   `shaders/truck.shd`. `visual-show --shader` aplica-o a todas as partes na
   criação; no jogo só aceita `models/ai/truck/shaders/truck.shd`. Uma falha
   libera a instância ainda não desenhada. `session-start` aplica o skin por
   padrão, e `--no-skin` mantém o fallback. O status informa `skinned_parts`.
   Não foi feito o `gfx_shader_precache` que o carro do jogo faz antes
   (possível carga tardia de textura no primeiro desenho).
4. **Recarga automática.** Uma mudança de `this` do `actors.car` oculta o visual
   na hora. A manutenção no gancho do loop (após as tarefas) libera quando é
   seguro e recria com o mesmo bloco de criação (modelo, skin, modo, alvo,
   offset), exibindo de novo. Ocultar manualmente cancela; uma recusa
   inesperada marca `fault=2`. O status informa `recreated` e
   `recreate_pending`. A fixture simula a recarga com uma segunda entidade; as
   instâncias criadas ficaram iguais às liberadas mais uma.
5. **`dev_env verify`**: build release x86 e nativo, bancada x86, bancada nativa, Clippy
   x86/nativo e rustfmt, com soma de passados/falhos (parser testado).
6. **Servidor `--echo`**: devolve a cada cliente a própria pose como
   `1000000000 + id` (cabe em u32), para testar rede, interpolação e desenho
   num só PC com `session-start --offset 400 0 0`. Teste próprio do eco;
   `run` sem eco mantém o comportamento anterior. `dev_env verify` final: x86
   41, nativos 47.
7. **Métricas de custo**: `draw_cost_avg_ns`/`draw_cost_max_ns` (EMA 1/16 e
   máximo do tempo das nossas chamadas ao motor por desenho externo) e
   `frame_interval_avg_us` (EMA do intervalo entre desenhos externos locais), em
   `visual-status`/`session-status`. Servem para medir o impacto no FPS no jogo;
   não substituem um profiler.

Ainda não comprovado no jogo: o skin (textura e custo do primeiro desenho), a
recriação depois de uma recarga real (a instância antiga atravessa o
carregamento até ser liberada), vários remotos e a interpolação com tráfego real
de LAN.

**Preparo da validação com eco — 28/10/2026:** a verificação dos testes passou,
mas o executável nativo release existente era antigo e recusava `--echo`.
Compilar os testes não o atualizava. `dev_env verify` agora também faz o build
release do workspace nativo, antes da bancada, mantendo o build x86 da DLL.
Os executáveis atuais ficam em `target/x86_64-pc-windows-gnu/release/`
(servidor/assets) e `target/i686-pc-windows-gnu/release/` (launcher/DLL).
O novo `verify` passou: 41 x86, 47 nativos, Clippy x86/nativo e rustfmt.
O servidor nativo atual aceitou `--bind 127.0.0.1:39000 --echo` e iniciou.
Os hashes dos cinco originais conferiram com o perfil. Não houve injeção nem
validação visual nesta preparação: o jogo ainda não estava aberto. O teste
real continua pendente da partida carregada; não promover esse preparo a
evidência de skin, suavização ou recarga na partida.

### M4/M5 — Primeiro eco real: material preto e falha na recarga — 28/10/2026

Processo histórico 15768, thread 3584; servidor nativo com eco em loopback.
`session-start` completou sete etapas com backup dos dois saves e hashes dos
cinco módulos originais. A DLL transmitiu/recebeu poses com quaternion; o
proprietário viu o caminhão copiando seu movimento, levemente atrasado e preto.
Uma amostra registrou 3.265 envios, 2.741 recebimentos, 16 amostras no histórico,
6.409 desenhos, `fault=0`, `foreign_calls=0` e acesso do item zerado entre frames.
Três partes receberam o shader; isso não comprovou sua aparência correta.
Custo EMA das chamadas próprias ~4,4 µs, máximo 205,6 µs, intervalo de frame EMA
~8 ms. Não houve medição de baseline nem RTT/perda/jitter: não converter esses
dados em garantia de impacto no FPS ou diagnóstico completo do atraso.
Os 100 ms de interpolação configurados podem contribuir para o atraso percebido.

**Falha:** ao tentar carregar um save existente com a instância ainda retida,
o jogo fechou. `prism3d.crash` e evento 1000 registraram `0xc0000005`, em
`ntdll+0x4e728`; a pilha inclui `RtlFreeHeap` e núcleo `+0x18165`, sem localizar
a origem da corrupção. Não concluir que foi o setter, double-free ou bug do
Windows só por esse registro. A recriação que espera outro `this` depois do
load não demonstra segurança durante o descarregamento do mundo antigo.
Os cinco originais e os dois saves conservaram os hashes. O processo terminou;
não foi possível executar teardown nele. Servidor de teste encerrado.
Relatórios/dump textual permanecem em `target/analysis/echo-session-*`, ignorados.

**Alterações para investigar e conter a falha:**

- `session-prepare-load`: oculta/libera o visual ANTES de carregar, com espera
  pelos flushes/contadores, mantendo sondas e rede. Depois da partida carregada,
  `visual-show` pode recriá-lo. Não apresentar esse contorno como reload
  automático corrigido. A bancada verifica instância ausente antes de trocar
  a entidade e parada limpa em seguida.
- `session-verify` foi movido antes de abrir o PID/resolver a DLL: a primeira
  tentativa após o crash falhava ao abrir o processo morto. Agora confere o
  backup após saída e detecta arquivo alterado; teste inclui encerrar a fixture
  antes dessas verificações. A conferência real após o crash retornou sete
  arquivos inalterados.
- Iluminação: `templates/car.tpl` usa material iluminado. O jogo instala
  `psm_set_light_callback` (`psm+0x38c0`, slot `game+0x160de0`) com callback
  cdecl `game+0x641c0` e contexto `this+0x84`; o callback faz tail-call thiscall
  do getter em `vtable+0x48`. `psm_draw` consulta-o sincronamente; instância
  nova tem contexto/callback zerados em dados `+0x5c/+0x60`. O caminho novo usa
  a cena atual apenas durante cada draw, verifica alvo no game/engine perfilado
  e página executável, e remove ambos os campos ao retornar. Não transfere nem
  retém propriedade da cena. Fixtures próprias verificam ABI/thread/contexto,
  callback durante o draw, remoção antes do free e descarte de cena inválida.
  O fornecimento de luz é uma hipótese de correção para o preto, não resultado
  visual já comprovado nem explicação comprovada do crash.

**Verificação desta revisão:** build release x86 e nativo, 41 testes x86,
47 nativos, Clippy x86/nativo e rustfmt passaram via `dev_env verify`.
A prova real da luz e do carregamento preparado ainda depende do novo teste.

**Próxima dependência:** nova prova de textura com o callback de luz, seguida
de liberação prévia/carregamento/recriação explícita e conferência dos arquivos.
Mapear uma fase anterior à destruição do mundo para restaurar a automação;
os passes e o evento de troca de entidade, sozinhos, são insuficientes. Uma
sonda de lifecycle deve comprovar ABI/thread/ordem antes de chamar liberações.
Dois PCs, vários remotos e estabilidade prolongada seguem pendentes.

**Reteste nativo com luz e carregamento preparado:** processo histórico 5956,
thread 18744. Novo `session-start` com backup de sete arquivos. O proprietário
dirigiu e confirmou que a aparência funcionou perfeitamente. Antes de carregar,
`session-prepare-load` liberou na primeira tentativa, sem recusas; status
`state=0`, `item=0`, `shown=0`, rede ainda ativa, hashes iguais. O proprietário
carregou o save existente; o processo permaneceu ativo. `visual-show` criou
outra instância, recebeu poses do eco com quaternion e desenhou novamente.
O usuário confirmou textura/movimento corretos após essa carga. Antes da
preparação foram 8.672 desenhos, `fault=0` e `foreign_calls=0`; após a recriação,
a captura inicial já tinha 43 desenhos e três partes com shader. EMA das
chamadas próprias ~4–5 µs, máximo 161,4 µs; continua sem baseline de FPS.

`session-stop --verify` também liberou na primeira tentativa, sem recusas:
cliente parado, três slots restaurados por leitura externa, dispatcher inativo,
sonda do loop removida e trabalho pendente/em execução zerado. Os sete arquivos
continuaram iguais. O usuário fechou normalmente; enumeração posterior não
encontrou o jogo, e `session-verify` ainda retornou sete inalterados com o PID
já morto. Servidor de teste encerrado. Relatórios em `lighting/` sob a pasta
local do eco. Este resultado valida luz e carregamento PREPARADO com recriação
explícita, não a retenção automática durante load. Luz e preparo mudaram nessa
revisão; o sucesso não isola a causa do crash anterior. A investigação da
transição pré-destruição continua necessária antes de repetir a automação.

### M4 — Sonda passiva de init/done antes do reload — 28/10/2026

Implementada `engine transition-attach/arm/status/report/remove` (comandos
41–46, sem mudar ABI v3/664 nem rede/configuração v1). A sonda intercepta
somente os dois virtuais existentes, repassa `this` e `phase` exatamente uma
vez e preserva EAX/LastError. Não chama init/done por conta própria nem libera
recursos. Instalação pela thread da janela, exigindo pump fixado; callbacks
de outra thread apenas repassam o original e incrementam diagnóstico.
O visual retido e esta observação são mutuamente exclusivos por guardas no
código. `session-backup` prepara cópias/hashes sem carregar a DLL.

**Candidato estático, específico dos hashes do perfil:** vtable de `actors.car`
`game+0x1257a0`, slots `+0x5c` → `+0x50f40` (init) e `+0x60` → `+0x515d0`
(done), thiscall com argumento na pilha e `ret 4`. A rotina done com fase zero
encadeia `+0x67510`, que chama `+0x68330` antes de `+0x7ddf0`.
`+0x68330` percorre o array de modelos em `this+0x200`, quantidade byte
`+0x300`, libera recursos e zera entradas; `+0x7ddf0` libera a física em
`this+0x1d0`. Isso identifica um candidato anterior à destruição dos recursos
desse ator, não comprova a fase global de destruição do mundo nem autoriza
chamar nossas liberações nesse callback.

O trace guarda até 128 eventos de entrada/retorno, argumento, thread, caller,
entidade, profundidade de chamada, flushes e leituras de cena (`+0x84`), física,
primeira entrada de modelo e quantidade. Também marca o primeiro draw após
armar e após cada grupo de callbacks. Memória ilegível vira campo ausente;
não desreferencia ponteiros lidos. Não bloqueia callbacks no mutex, não aloca
no trace e sinaliza perda por contenção/truncamento. Rearmar muda a geração;
registros anteriores nunca são sobrescritos dentro de uma geração.

Instalação confere hashes, páginas executáveis, alinhamento e ambos os slots
antes da primeira escrita. Remoção restaura somente slots ainda pertencentes
à sonda e sua proteção; conflito retém a DLL e permite nova tentativa. Parada,
detach e remoção do pump removem a sonda, com referências de forwarding válidas
até o fim do processo. `session-stop` passa a conferir cinco slots externos
(draw/init/done/render/flush). A bancada própria acrescenta um ciclo de
teardown/init com marcador de modelo, argumento/retorno/LastError, nested,
foreign, limite de trace, geração antiga, conflito de ownership, restauração
na parada e continuidade dos originais; também verifica exclusão com visual
e backup sem injeção. `dev_env verify`: **43 testes x86, 48 nativos**,
build release x86/nativo, Clippy dos dois alvos e rustfmt passaram.
Log local ignorado: `target/analysis/transition-verify.log`.
Logs do helper ficam somente em `target/analysis/`.

**Captura real, sem visual:** processo histórico 21192, thread 6148. Backup
de cinco módulos e dois saves em
`target/analysis/transition-20261028-01/backup`. O proprietário manteve a
partida em foco, aguardou no menu, carregou um save existente e confirmou
"tudo normal". Trace completo com **16 eventos**, geração 1, `flags=1`
(ativa, sem perda/truncamento), quatro init, três destroy, zero chamadas de
outra thread, profundidade 1 nos callbacks e todas as quatro leituras válidas.

| Ordem efetivamente observada | Entidade | Física (`+0x1d0`) |
| --- | --- | --- |
| Primeiro draw, flush 3 | antiga `0x0c512eac` | `0x12d0ec00` |
| Init `-10,-1,0,10`, enter/return, flush 11618 | nova `0x0d9e33d8` | zero em todos esses eventos |
| Destroy `-10`, enter/return, flush 11618 | antiga | ainda `0x12d0ec00` |
| Destroy `0`, enter → return, flush 11618 | antiga | `0x12d0ec00` → zero |
| Destroy `10`, enter/return, flush 11618 | antiga | zero |
| Primeiro draw seguinte, flush 11621 | nova | `0x0f2f7290` |

Callsites de retorno: `p3shared+0xc4ce` dentro de `init_eblock` e
`p3shared+0xc84c` dentro de `_destroy_eblock`. Os símbolos exportados
identificam `init(int)`/`destroy(int)`, ambos retornando inteiro; o nome `done`
do diagnóstico refere-se a esse destroy por fase, não ao destrutor que libera
o objeto. O relatório passa a oferecer `phase` com sinal e `argument` com os
bits originais. A bancada passa a reproduzir também essa ordem com entidades
distintas e fases negativas, sem supor destroy antigo antes de init novo.
Após incorporar essa sequência e a apresentação assinada das fases,
`dev_env verify` voltou a passar: **43 x86, 48 nativos**, builds release,
Clippy x86/nativo e rustfmt; saída zero. Log em
`target/analysis/transition-20261028-01/verify-after-real-final.log`.

As cenas eram distintas e a nova já existia durante o init: `0x12ece430`,
enquanto a antiga manteve `0x0f3ea508` até o último callback observado.
Não houve flush adicional entre esses grupos de init/destroy. A primeira
entrada de modelo era zero antes e depois, apesar de quantidade 16; esta
captura não prova em qual fase cada entrada não nula foi liberada, nem que
quantidade preservada indique recursos vivos. Também não há instância própria
retida neste teste para comprovar seu lifetime.

`session-stop --verify`: sem recurso a liberar, sem recusas, cinco slots
restaurados, worker parado, dispatcher inativo, pump removido, nenhum trabalho
pendente/em execução e **sete hashes iguais**. Sonda em state 2, captura
inativa (`flags=0`), mesmos 16 eventos. O proprietário confirmou fechamento
normal; enumeração não encontrou `prism3d` e a verificação pós-saída repetiu
sete inalterados. Não há DLL residente desse processo. Captura/backup/logs são
locais e ignorados; nenhuma alteração em originais ou saves.

**Candidato anterior, apenas estático:** `p3shared+0xc600`
(`entity_proc_t::destroy_eblock`) agenda o item na lista do processador em
`+0x3ac`; `+0xbe23` chama `_destroy_eblock` ao consumir essa lista. O jogo
importa esse agendamento em `game+0x1242b4`, com referências em
`+0x4ff58,+0x69de6,+0x69e97,+0x6c197,+0xf2fee`. Há também destruição imediata
no rollback de init (`p3shared+0xc5d6`). Uma eventual nova sonda deve distinguir
solicitação, execução e rollback, e a que bloco/cena cada chamada pertence.
Não foram interceptados ou chamados esses exports pelo mod nesta prova.

**Dependência ainda aberta:** a captura provou thread/argumentos/ordem por ator,
mas o init do novo ator ANTES do destroy do antigo impede usar simplesmente
`car::destroy(0)` como começo universal da recarga. Observar a solicitação de
destruição e/ou início global do carregamento, incluindo rollback, e relacionar
com o consumo de filas antes de definir liberação antecipada/recriação.
Não repetir reload com instância retida com base apenas nesta sonda.
O procedimento preparado continua válido.

### M4 — Observar blocos antes da destruição por ator — 28/10/2026

Entregue `engine transition-blocks` (comandos 47/48, mesma ABI v3/664).
Amplia a sonda de transição já instalada, antes de armar, com os imports do
jogo para `entity_proc_t::init_eblock` (`game+0x124560` → `p3shared+0xc270`)
e `destroy_eblock` (`game+0x1242b4` → `p3shared+0xc600`). Ambos thiscall com
argumento ponteiro, bool em AL e ret4. Verifica hashes do jogo/provedor,
executabilidade e os dois slots antes de escrever. Não chama nenhum export
adicional: apenas repassa cada chamada original uma vez, na thread original,
preservando argumento, retorno e LastError. A regra de exclusão com visual
retido continua em vigor; ainda não automatiza liberação.

A mesma timeline agora distingue entrada/retorno de init de bloco,
SOLICITAÇÃO de destruição e fases do ator aninhadas. Inclui item, dados/contexto,
contadores diretos, dois words crus da fila do processador, retorno booleano
e último caminhão desenhado. Leituras de cena/física/modelos nas linhas de
bloco referem-se a esse último caminhão, sem fixar ou presumir sua validade.
Também captura até 16 entradas cruas de modelo, com flag de quantidade acima
do limite, para não inferir liberação apenas a partir da primeira entrada nula.
Leituras inválidas ficam ausentes, sem desreferência direta dos valores lidos.

Trace limitado a 512 eventos, sem heap nem bloqueio nos callbacks; rearme só
atualiza metadados, evitando temporário grande na stack do jogo. Extensão deve
ser instalada antes da captura e com o mesmo modo fixture/jogo. Remoção
restaura os quatro slots na ordem inversa e recusa sobrescrever outro dono;
retry também restaura slots que já voltaram ao original. Forwarding permanece
válido até a saída do processo, pela fixação do pump. `session-stop` verifica
sete slots por leitura externa (draw/init/destroy, dois imports e render/flush).

A fixture acrescenta processador e itens próprios, fila simulada, bool com
EAX superior sujo, chamada de outra thread, init aninhado, solicitação antes
do init novo, execução posterior do destroy antigo e init false com rollback
somente do novo. Testes conferem identidade, profundidade, retorno/LastError,
preflight sem mutação, ownership de import/vtable, restauração, continuidade
dos originais, serialização e limites de modelos/trace. `dev_env verify` passou:
**44 testes x86 e 49 nativos**, build release x86/nativo, Clippy dos dois alvos,
rustfmt e saída zero. Log local `target/analysis/transition-blocks-verify.log`.
**Primeira captura real:** processo histórico 11496/thread 8456, proprietário
confirmou recarga normal. Snapshot preservado de **70 eventos**, 20 init de
bloco, três solicitações de destroy, quatro init/três destroy do ator e dez
marcadores de primeiro draw. Uma só thread, profundidade máxima 3, 30 pares
enter/return correspondentes e completos, todos os bools true, `flags=1`, sem
foreign, perda, truncamento de trace ou de modelos. Artefatos ignorados em
`target/analysis/transition-blocks-20261028-01/`.

Primeiro novo processador/cena observado em `block_init_enter`, caller
`game+0xcc68a`, flush 13272. Em seguida, `game+0xcc8be` inicializou outro bloco
na mesma cena e as fases `-10,-1,0,10` do novo ator ficaram aninhadas nesse
init, profundidade 2; fase zero criou cinco blocos adicionais em profundidade 3.
Somente depois veio destroy do ator antigo, no mesmo flush. As doze entradas
antigas não nulas (índices 1,2,6–15) e a física foram zeradas durante destroy(0),
com quantidade declarada ainda 16. Primeiro draw novo no flush 13275, com doze
entradas e física novas. Contagem declarada não equivale a recursos vivos.

As três solicitações registradas apareceram DEPOIS da troca, caller
`game+0xf2ff4`, na cena nova; nenhuma delas precedeu o destroy do ator antigo.
Logo, esse import não identificou a solicitação global responsável pela troca.
Não usar qualquer destroy_request como gatilho de reload: há tráfego normal
de blocos enquanto se joga. Nenhum rollback real foi visto; esse caso continua
validado somente na fixture.

Na parada, status chegou a 91 registros (24 init de bloco/seis solicitações),
ainda sem foreign/perdas/truncamento. Esse total posterior foi consultado como
status; só o snapshot inicial de 70 registros foi preservado integralmente.
Sete slots restaurados por leitura, dispatcher/pump removidos, worker parado,
sem operações pendentes/em execução e sete hashes iguais. Proprietário fechou
normalmente; enumeração confirmou saída e verificação pós-saída repetiu sete
inalterados. A tentativa de relatório completo posterior ao fechamento foi
recusada; não reconstruir dados ausentes a partir do status.

**Nova pista estática:** os callers correspondem a `game+0xcc5f0` e `+0xcc8a0`;
essas rotinas são chamadas pelo handler em `game+0xd3570`. O handler começa
consultando `cmd_argv` pelo binding `game+0x160930` (retorno `+0xd3596`) e
despacha comandos textuais; há ramos de criação, carga e encerramento que
trocam `this+0x14c`. O núcleo exporta `cmd_argv` em `prism3d+0xe210`.
Esse começo é um candidato anterior à troca; falta observar seu contrato,
identificar o comando real e provar a correlação antes de qualquer adiamento
ou liberação automática.

**Limites:** os imports interceptados pertencem somente ao jogo; chamadas
diretas dentro de p3shared ou por outros módulos podem não aparecer. Solicitar
destroy não comprova início global da recarga nem seu término. Correlacionar
blocos, cenas, calls do ator e flushes antes de escolher uma fase de liberação.
Não repetir reload com visual retido com base apenas nessa observação.

### M4 — Conclusão da extensão de comandos do commit 9587a15 — 28/10/2026

O commit recebido iniciou os comandos 49/50 (`transition-commands` e fixture),
o shim cdecl de `cmd_argv` e a inclusão do slot na verificação da sessão, mas
não completou testes/documentação. A revisão manteve a ABI v3/664 bytes e
completou o bloco, sem ativar liberação automática nem alterar os originais.

O callsite foi conferido estaticamente na instalação local: no handler
`game+0xd3570`, ECX ainda contém o receptor ao chamar `game+0x160930`, com retorno
em `+0xd3596`. O destino do perfil `prism3d+0xe210` lê um global e retorna o
ponteiro argv, sem argumentos na pilha. A fixture comprova forwarding único,
retorno EAX/ponteiro, identidade de entrada ECX, pilha e LastError. A sonda
filtra caller e thread e registra antes do despacho efetivo do comando.

A captura de nome agora é limitada, testável no host e lê só `argv[0]`:
32 bytes incluindo terminador, sem consultar argv[1] ou quaisquer argumentos.
Falha de leitura/overflow/ponteiro nulo ou ausência de terminador descarta o
prefixo parcial. O JSON distingue `complete`, `unreadable`, `unterminated` e
`invalid_utf8`, sem inventar um comando. O evento também lê `handler_scene`
em `this+0x14c`; esse campo não é confundido com dados de bloco. As demais
leituras referem-se ao último ator observado e não fixam sua memória.

Foram adicionados testes de instalação inválida/duplicada, caller não
perfilado, thread estrangeira, comando antes de blocos e fases do ator,
ponteiros inválidos/nulos, string no limite/sem terminador, conflito no quinto
slot e restauração no detach. A fixture continua encaminhando chamadas depois
da remoção sem acrescentar eventos. A verificação de sessão abrange agora
oito slots, incluindo `cmd_argv`; os cinco hooks de transição são removidos.
Também foram corrigidas as falhas de formatação/Clippy do commit recebido.

O novo teste expôs uma falha executável na fixture: `push {site}` carregava a
palavra no endereço do símbolo (os bytes `ret`/padding), não seu endereço.
A captura SEH local mostrou salto para `0x0f2e66c3`, a palavra formada por esses
bytes, durante o comando da fixture. Foi substituído por `lea`/`push` de um
registrador e corrigido o alinhamento GNU x86/limpeza da continuação. A recusa
posterior de enumeração de módulos era consequência da saída desse processo,
não prova de falha de permissão. Isso não explica a falha histórica do jogo ao
recarregar com visual retido; são caminhos diferentes.

Validação desta revisão no **Fedora**: **46 testes nativos e 46 testes x86
sob Proton passaram**, build release cruzado, Clippy nativo/x86 e rustfmt.
Inclui a bancada completa, com o teste de transição exercitando também o quinto
hook. Logs em `target/analysis/command-*` (ignorados).
Não houve teste novo no jogo, e as validações Windows anteriores permanecem
separadas. O processo do jogo foi procurado antes do build e não estava ativo.

**Limite:** o `load` da fixture é simulado; não se conhece ainda o comando real
da recarga nem se esse ponto precede todas as liberações globais. A extensão
continua passiva, incompatível com visual retido. Manter `session-prepare-load`
antes de carregar e `visual-show` depois. Próximo bloco: analisar a timeline
de forma reproduzível, relacionando comandos, cenas, pares enter/return,
rollback e primeiro draw; nunca converter observação isolada em autorização
para liberar recursos do motor.

### M4 — Captura persistida e análise offline da timeline — 28/10/2026

Entregue o próximo bloco após completar `transition-commands`: o launcher
aceita `engine transition-report --output ARQUIVO.json`, coleta todos os
registros da geração antes de criar o arquivo e recusa sobrescrever capturas.
O JSON preserva summary/records e acrescenta análise ou erro de análise.
Sem a opção, a saída JSON no terminal permanece disponível. A pasta pai deve
existir; falhas de I/O são reportadas, podendo deixar um arquivo novo incompleto.

O novo binário Rust nativo `transition_report CAPTURA [--output ANALISE]`
funciona após o processo encerrar, sem acesso ao jogo, assets ou DLL. O módulo
portável `transition_analysis` usa os mesmos eventos do launcher, com limite
de 8 MiB/512 registros e validação de esquema/geração/contagem. Relaciona
atores/cenas, fases assinadas, chamadas aninhadas, resultados false de init de
bloco, leituras antes/depois de done(0), comandos e primeiro draw subsequente.
Expõe índices dos registros, ordem do init novo/done antigo e diferença de
flushes entre o retorno de done(0) antigo e o primeiro draw diferente.

Perdas, truncamento, thread inesperada, pares/profundidades inconsistentes,
chamadas abertas e nomes incompletos são ressalvas explícitas. Contadores de
chamadas estrangeiras são cumulativos e tratados conservadoramente. Leituras
ausentes e modelos acima de 16 não se tornam prova de limpeza; retorno false
não é prova de rollback completo. Comandos delimitam somente intervalos
cronológicos, sem inferir causa ou duração do handler. Relatórios anteriores
sem comandos continuam aceitos, sem inventar o início ausente do load.

`structurally_complete` avalia somente a estrutura do snapshot. Mesmo uma
captura íntegra mantém `automation_validated=false`: não prova cobertura
global, ownership, consumo de todas as referências ou momento seguro para
teardown. Endereços podem ser reutilizados entre gerações. A implementação
não chama callbacks extras nem altera ABI/hooks ou o caminho manual seguro.

Fixtures próprias exercitam init novo antes do destroy antigo, fases negativas,
falha do bloco novo mantendo ator antigo, nomes incompletos, perdas/limites,
leituras ausentes e fronteiras de comandos aninhados. A bancada x86 também
salva uma captura, recusa sobrescrita sem alterar seus bytes, encerra o host e
executa o analisador offline. Não depende de assets comerciais.

Validação no **Fedora**: **51 testes nativos e 51 x86 sob Proton passaram**,
incluindo persistência/recusa de sobrescrita e análise após a saída do host.
Build release nativo e cruzado x86, Clippy nos dois alvos e rustfmt passaram.
Logs em `target/analysis/timeline-*` (ignorados). Não foi executado o jogo;
essas verificações não substituem uma recarga real no Windows ou no Proton.

**Próximo:** capturar `transition-commands` numa recarga real SEM visual retido,
salvar o snapshot antes da saída e analisar comandos/cenas/ordem com a nova
ferramenta. Provar qual comando real precede as liberações globais antes de
propor automação. Nenhuma nova validação no jogo neste bloco; LAN entre dois
PCs e sessões longas continuam pendentes. Manter o preparo manual do load.

### M4 — Primeira captura real de comandos e análise após saída — 28/10/2026

Executado o procedimento `transition-*` no jogo Steam original sob Proton no
Fedora, **sem servidor, rede conectada ou visual retido**. O proprietário
carregou a partida, dirigiu, voltou ao menu e recarregou o save existente.
Os hashes do núcleo/jogo corresponderam ao perfil; bases e slots foram
resolvidos no processo atual (PID Windows histórico 396, thread 400). Backup
do prefixo antes de abrir a Steam, mais cópia/hash de um save e hashes de cinco
originais pelo `session-backup`. Esses caminhos e artefatos ficam ignorados.

O snapshot inicial tem **48 registros, 21 pares completos**, sem perdas,
truncamento ou chamadas estrangeiras, profundidade máxima 3. Na geração 1:

- Eventos 4/5: `game_menu` e `map`, ambos com nome completo, mesmo handler e
  cena antiga legível (`handler_scene` igual à cena do último ator observado).
  Não há fase de ator capturada entre esses dois comandos. Isso descreve este
  intervalo; não prova que o menu não faça trabalho por caminhos não observados.
- Depois de `map`, o primeiro init de bloco usa o processador da cena nova;
  primeiro init do novo caminhão no evento 15. Fases `-10,-1,0,10`, seguidas
  por done do antigo nos eventos 34–39 (`-10,0,10`). São identidades distintas.
- No par done(0), eventos 36/37, a física passa de não nula a zero e todas as
  entradas de modelo lidas ficam nulas. Isso não comprova liberação de todos
  os heaps, caches ou referências do motor.
- Primeiro draw do novo ator no evento 42, três flushes depois do retorno de
  done(0). A solicitação de destroy de bloco capturada vem depois da troca,
  ainda na cena nova, e continua sem representar o início global do load.

O comando real desta recarga é **`map`**, não o `load` simulado na fixture.
A leitura estática do perfil confirma o ramo comparado em `game+0xd3666`
com a string `map` (`+0x14be44`), seguido pelo tratamento da cena antiga em
`+0xd36be`/`+0xd36d1` e pela criação/carga nova. A captura em `+0xd3596`
precede esse ramo. Isso torna o início do handler um candidato concreto;
não comprova que nenhum outro callback já iniciou trabalho global, que todos
os tipos de carga passam por ele ou que seja seguro executar `psm_free` ali.

Após `session-stop --verify`, foram preservados **61 registros integrais,
27 pares**, estado removido, flags/foreign zerados, 19 init de bloco/uma
solicitação, quatro init/três done do ator e os mesmos dois comandos.
Todos os retornos de bloco capturados foram true; nenhum rollback real foi
validado. Oito slots restaurados por leitura externa, dispatcher removido,
pump restaurado, worker parado, pending/running/inflight zerados, visual
ausente e nenhum pacote de rede enviado/recebido. **Seis arquivos iguais**
(cinco originais/um save), sem extrapolar o total de sete de outras sessões.

O proprietário confirmou a conclusão da condução/fechamento após a limpeza,
sem relatar falha. Enumeração posterior confirmou ausência do jogo, e
`session-verify` pós-saída repetiu os seis hashes iguais. O analisador nativo
releu o snapshot final após a saída e retornou a mesma estrutura sem ressalvas;
`automation_validated` permaneceu false. Capturas/relatórios locais em
`target/analysis/transition-command-fedora-20261028-01/` (ignorados), incluindo
snapshots inicial/final e análises antes/depois da saída. Ao contrário do
registro anterior de 91 eventos, os 61 desta sessão foram preservados.

Esta etapa validou a captura real, sem alterações de código ou novo build.
A bancada de 51 testes nativos/51 x86 do bloco anterior continua sendo a
evidência automatizada; não foi necessário repeti-la para documentação.
Não há processo do jogo com DLL residente ao concluir esta validação.

**Próximo:** avaliar o contrato de liberação no início observado de `map`,
incluindo thread, profundidade, estado da fila/render e referências da instância.
Preparar uma política testável de recusa/recuperação antes de ligar liberação
automática; determinar como impedir a carga se não for seguro liberar, sem
inventar ABI de adiamento ou chamar o motor recursivamente. A captura atual é
passiva e incompatível com visual retido. Manter `session-prepare-load` antes
de carregar e `visual-show` depois. Não repetir a recarga retida que falhou.
Matriz de outros menus/cargas, LAN entre dois PCs e sessões longas pendentes.

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

### Decisões de produto confirmadas — 28/10/2026

O proprietário aprovou a direção de competição econômica entre transportadoras
e acrescentou as regras abaixo. São requisitos de desenvolvimento; a aprovação
não comprova implementação nem substitui os testes de cada marco.

**Prioridade LAN, hospedagem externa futura.** A migração para uma máquina
externa/VPS é um adendo de arquitetura, não uma entrega prioritária. Manter o
servidor Rust sem dependência do processo do jogo, de GPU ou de jogador-host;
endpoint configurável e contratos separados para conexão, identidade, regras
e persistência. Evoluir esses contratos quando necessários à LAN/economia,
evitando acoplamentos que obriguem reconstruir todo o projeto depois. Não
comprar/hospedar agora nem antecipar banco, painel ou serviços sem necessidade.
Internet não é garantida por trocar o IP: autenticação, reconexão, proteção
contra abuso e operação/recuperação serão exigidas antes da exposição pública.

**Uma única carreira no mod.** Usar `Become a Millionaire` como base econômica;
não oferecer os outros quatro cenários como opções no fluxo do mod. Antes de
criar um save, apresentar meta financeira configurável e checkbox `Sem limite`.
Com o checkbox ativo, o campo da meta fica desabilitado e o alcance de um valor
financeiro não termina a carreira nem declara vitória. Economia, fretes,
empresas e concorrência continuam ativos; não significa dinheiro infinito.
É uma variante de configuração da mesma carreira, não um Free Mode separado.
Gravar a regra versionada junto ao save multiplayer e recuperá-la ao carregar;
todos os participantes da mesma sessão usam a mesma configuração. O valor
padrão pode conservar 1.000.000 enquanto o usuário escolhe outra meta.
Ainda investigar qual grandeza dispara a vitória original (caixa, patrimônio
ou receita) antes de definir o texto exato da meta e adaptar a condição.
Desabilitar a regra original de 1 milhão exige alterar o comportamento, não
somente o texto do menu. Não modificar saves existentes silenciosamente.

**Empresas e economia compartilhada.** Após a prova visual LAN, introduzir
identidade durável de jogador/empresa (não IP/porta), nome/cor/sede, fretes
compartilhados com aceite exclusivo, despesas/pagamentos validados e ranking.
Depois ampliar frota, motoristas IA, licitações e cooperativas; concorrentes
IA podem ser uma configuração futura da mesma carreira. Investigar eventos
e estruturas econômicas do jogo por leitura antes de integrar callbacks.
Primeiro contrato vertical: aceitar → transportar → entregar → receber uma vez,
com recuperação de desconexão e reinício, antes de expandir a gestão.
Separar snapshots descartáveis de comandos confirmados, IDs de operação,
deduplicação e transações persistentes. Um relato do cliente de que entregou
não comprova sozinho uma entrega válida; explicitar os limites da verificação
enquanto a física continuar local. Posição em rede é temporária, mas a retomada
precisa de checkpoint coerente de veículo/carga/contrato. SQLite é candidato
inicial, não dependência já escolhida; persistência não precisa rodar a 60 Hz.

**PT-BR e reais, etapa futura.** Traduzir o jogo integralmente e adotar `R$`
como moeda apresentada. Inventariar menus, HUD, mensagens, ajuda/tutorial,
textos em imagens e demais conteúdos de idioma; verificar fontes, acentos,
codificação e limites de layout. Inventariar eventual áudio dependente de
idioma antes de fechar seu tratamento. Formatação brasileira e unidade de
conta devem ser separadas da lógica econômica. Ainda definir se os valores
numéricos serão mantidos ou rebalanceados; não assumir conversão cambial real.
Planejar valores exatos em unidades monetárias menores na economia própria e
adaptar explicitamente o formato do motor, sem arredondamentos silenciosos.
Regras financeiras do save não podem mudar por trocar idioma/formatação.
Distribuir somente textos/recursos próprios e transformação local dos
originais; não versionar pacotes comerciais reempacotados.

**Inspeção preliminar local, somente leitura:** a listagem do `aa/base.scs`
confirmou `defs/scenario_base.def` e os cinco arquivos em `defs/scenarios/`,
incluindo `become_millionaire.def`; o patch contém uma definição de cenário.
A leitura posterior com o binário atual confirmou `win:"money:1000000"` no
cenário e `none:0` como condição sem vitória na definição base. A interface de
novo jogo está em `ui/mnewgame.esc`, com handler `ui.handler.menu.newgame`.
Isso fornece candidatos concretos para meta/checkbox, sem comprovar a semântica
de patrimônio/dívida, a persistência ou que somente editar o arquivo baste.
Não houve alteração/extração aplicada aos arquivos do jogo.

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
- Preservar servidor independente da instalação do jogo e da interface; separar
  identidade durável de conexão UDP e regras de apresentação/idioma. Hospedagem
  externa é evolução futura, sem deslocar a validação LAN atual.
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
  Progresso: rotação opcional (quaternion unitário, tolerância 2%, v1
  compatível) entregue e testada na bancada e no eco real; tick, modelo e velocidade pendentes.
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

O primeiro visual PSM seguindo o jogador já foi visto na partida, sem textura.
O caminho escolhido desenha modelos, sem inserir entidades físicas no mundo.
Agora a dependência é validar as implementações de rede/skin/recarga no jogo e
em dois PCs; inserção como entidade continua sem contrato e não é pré-requisito
para o MVP visual sem colisão.

- [ ] Investigar criação de veículo/modelo, atualização de transform e remoção,
  usando somente a instalação local e registrando conclusões próprias.
  Progresso: o caminho escolhido é visual, não entidade. Uma instância PSM é
  desenhada após o `draw` do jogador, com transform por push/translate/qrotate
  e remoção segura após flush (validado no jogo). A inserção como entidade do
  mundo continua sem contrato.
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
- [x] Validar setter PMD privado e descritor PSM, com bindings/hashes e recarga.
- [x] Criar/liberar instância PSM privada sem draw: 18 ciclos reais, incluindo
  recarga; fixture com shaders compartilhados, falhas e cancelamento.
- [ ] Confirmar ABI/ownership de inicialização, inserção, transform e remoção
  do mundo antes de ampliar as chamadas `unsafe`.
- [x] Criar fachada Rust para o ciclo privado, mantendo offsets e chamadas
  do motor fora do núcleo de rede; ampliar somente com operações comprovadas.
- [x] Encaminhar diagnóstico e lifecycle por fila limitada até o ponto observado
  do loop, fora de callbacks/reentrância; nenhum objeto é manipulado pelo UDP.
- [x] Criar primeiro um único veículo visual de teste a distância controlada,
  usando assets originais disponíveis localmente e física/colisões desativadas.
  Validado em 28/10/2026: `visual-show` desenhou o truck.psm de IA 4 m ao lado,
  sem textura (fallback), liberado com segurança e recriado após recarga.
- [x] Aplicar shader e iluminação emprestada da cena ao visual, com callback
  removido após cada draw; aparência confirmada no jogo, inclusive após
  liberação prévia/carregamento/recriação explícita. Não valida reload retido.
- [ ] Mapear jogador → entidade, atualizar transform, remover na saída e limpar
  referências ao carregar outro mapa/save ou encerrar a sessão.
  Progresso: ID remoto → visual (`--player ID|all`), pose interpolada,
  sem desenho após 1 s, recriação automática na troca da entidade local e
  `session-stop` com liberação segura. Eco no jogo e carregamento preparado
  passaram; retenção/recriação automática durante carga falhou no primeiro
  teste e continua em investigação. Faltam tráfego LAN real e saída de pares.
- [ ] Confirmar que mudar o remoto não muda o caminhão local nem os dados do save.
  Progresso: nas sessões de 28/10 os hashes de originais e saves ficaram iguais
  e o caminhão local não foi afetado, mas o visual seguia o próprio jogador.
  Repetir com remoto real; `session-start --backup` + `session-stop --verify`
  automatizam a conferência.
- [ ] Usar overlay/indicador apenas como diagnóstico se necessário; isso não
  substitui a entrega de um caminhão remoto no mundo 3D.

**Concluído quando:** dois jogos em máquinas distintas mostram o visual remoto
correto e ele aparece/desaparece com a sessão sem duplicatas ou referências inválidas.

### M5 — Movimento remoto suave e apresentação

- [x] Manter buffer por jogador com timestamps e limite de memória (16 amostras,
  tempo de recepção local; bancada).
- [ ] Interpolar posição e orientação; confirmar convenção de quaternion e tratar
  rotações equivalentes para não girar pelo caminho longo. Implementado (lerp +
  nlerp pelo arco curto, bancada); falta confirmar a convenção no jogo.
- [ ] Definir atraso de interpolação, extrapolação curta e comportamento quando
  os dados param de chegar. Medir em vez de escolher tolerâncias cegamente.
  Valores iniciais na bancada: atraso 100 ms, extrapolação ≤200 ms, sem desenho
  após 1 s; ainda precisam de medição na LAN.
- [x] Separar teleporte/correção de movimento normal; não atravessar o mapa com
  interpolação longa após reconexão ou carregamento (velocidade implícita
  >20.000 cm/s faz snap, na interpolação e na extrapolação; bancada).
- [ ] Sincronizar modelo/cabine, reboque e sinais visuais conforme descoberta:
  direção, rodas, freio, faróis e setas, sem exigir todos para o primeiro teste.
- [ ] Tratar visibilidade, LOD, nomes e estados de desconexão sem poluir a interface.
- [ ] Medir uso de CPU, memória, rede e efeito no FPS com dois e mais jogadores.

**Concluído quando:** dirigir e observar curvas/frenagens na LAN é coerente e
suave, e perda temporária de pacotes não produz movimentos indefinidos.

### M6 — Estado do mundo e regras de gameplay

- [ ] Restringir o fluxo do mod à carreira `Become a Millionaire`, sem apagar
  cenários/arquivos originais. Confirmar a origem da lista nos menus e das regras.
- [ ] Implementar meta configurável antes de criar o save e checkbox `Sem limite`;
  validar limites de entrada, salvar/restaurar a regra e impedir divergência
  entre participantes. Identificar a grandeza econômica e substituir o gatilho
  original de vitória, inclusive para metas menores/maiores e sem limite.
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
- [ ] Provar um contrato compartilhado com aceite exclusivo, validação da entrega
  e pagamento único; testar duplicação, desconexão e reinício com fixtures próprias.
- [ ] Introduzir empresas dos jogadores e ranking com identidade persistente;
  depois ampliar compra/manutenção de frota. Motoristas IA, licitações,
  cooperativas e concorrentes IA configuráveis dependem dessa base.
- [ ] Persistir regras da carreira e checkpoints de jogador/veículo/carga/contrato,
  separados dos snapshots de movimento e dos saves normais. Banco/migrações e
  formato versionado serão definidos com o contrato; não inferir suporte atual.
- [ ] Isolar dados multiplayer dos saves de uso normal, com backup/restauração
  e tratamento de desconexão durante eventos importantes.

**Concluído quando:** as regras escolhidas estão implementadas e testadas; a
interface informa claramente quais sistemas são compartilhados e quais são locais.
Colisão e economia compartilhadas podem ser marcos posteriores ao MVP visual.

### M7 — Pipeline de assets e modificações distribuíveis

- [ ] Tradução integral PT-BR (futura): inventário de conteúdo de idioma,
  catálogo próprio, fontes/acentos, layout e validação na partida; registrar
  cobertura e lacunas, incluindo textos embutidos em imagens e eventual áudio.
- [ ] Moeda R$ (futura): definir manutenção/rebalanceamento dos valores, aplicar
  apresentação brasileira em todas as telas e conferir persistência/cálculos.
  Não apenas trocar um símbolo isolado nem depender de cotação cambial real.
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
  Progresso: protocolo (rotação válida/inválida, v1 sem rotação), eco do
  servidor, interpolação/teleporte/extrapolação, fonte `drawn`, visual remoto
  ponta a ponta e sessão com backup têm testes próprios; `dev_env verify` roda
  tudo. Faltam mudança de versão e sessões com várias conexões.
- [ ] Simular perda, duplicação, jitter, atraso e reordenação com uma ferramenta
  Rust controlada; registrar parâmetros e resultado reproduzível.
- [ ] Testar sessão longa, menus, reload, encerramento abrupto e reconexão, com
  verificações de vazamento e remoção de entidades.
- [ ] Ter logs com versão/perfil/estado e coleta opcional de métricas, sem dumps
  completos ou conteúdo do jogo nos relatórios públicos.
  Progresso: `session-status` (cliente, visual, draw, remotos) e métricas de
  custo do desenho/intervalo de frame; logs com versão/perfil pendentes.
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

Hospedagem externa não é prioridade atual. Os contratos de arquitetura da
seção 3 devem permitir evoluí-la sem refazer a integração do cliente; os itens
abaixo continuam futuros e exigem validação própria.

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

Prioridade imediata: testar o visual já implementado com eco no jogo (skin,
recarga automática, suavização e custo) e depois com dois PCs na LAN. Não
refazer a descoberta do primeiro visual já comprovado. Após essa prova, seguir
com identidade/reconexão e um contrato econômico completo, expandir empresas
e a carreira de meta configurável. PT-BR/R$ e hospedagem externa são futuras;
não é preciso concluir toda a economia ou sincronizar todo o trânsito para
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
   anterior era 1.98.1; a migração 1.99.0 foi testada no Fedora/Proton e na
   bancada Windows nativa em 28/10/2026. Esses comandos não
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

5. Leia no README a seção **Retomada no Windows com Rust 1.99.0**. Instale os
   componentes e compile `dev_env` com o bootstrap descrito. O helper Rust
   seleciona GCC/GNU ld e prepara o cache LLVM por versão sem gravar caminhos
   particulares nos fontes. Não copie a `.cargo/config.toml` desta máquina.
6. Compile e valide:

```powershell
.\target\debug\dev_env.exe windows --native
.\target\debug\dev_env.exe windows
cargo fmt --all -- --check
.\target\debug\dev_env.exe windows --native --clippy
.\target\debug\dev_env.exe windows --clippy
.\target\debug\dev_env.exe windows --native --tests
.\target\x86_64-pc-windows-gnu\debug\test_host.exe target/windows-native-tests.json
.\target\debug\dev_env.exe windows --tests
.\target\i686-pc-windows-gnu\release\test_host.exe target/windows-tests.json
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

**Próxima entrega:** comprovar uma fase de lifecycle anterior à destruição do
mundo e automatizar a liberação/recriação nessa transição. A textura com luz
e o carregamento PREPARADO passaram no jogo, com confirmação visual, limpeza
e hashes iguais. O primeiro eco sem luz ficou preto e fechou ao carregar com
o visual retido; a causa não foi isolada. Ver registro "Primeiro eco real".
Antes de abrir/carregar um save,
executar `session-prepare-load` e aguardar `prepared=true`; após carregar,
recriar via `visual-show`. Não repetir retenção durante load como se a
recriação automática estivesse validada. Mapear a fase pré-destruição para
retomar essa automação. Ambos os processos históricos já terminaram e os arquivos
conservaram os hashes; reenumerar antes de qualquer nova ação.

**Atualização da sonda pré-teardown:** `transition-*` foi validada no processo
histórico 21192/thread 6148, com 16 eventos íntegros. Init do novo ator nas fases
`-10,-1,0,10` ocorreu ANTES do destroy do antigo em `-10,0,10`. Física antiga
zerada em destroy(0); primeiro draw novo três flushes depois. Essa prova por
ator não identifica o começo global do load. Próximo: observar agendamento de
destruição (`game` importa `destroy_eblock` em `+0x1242b4`) e/ou entrada global
de carregamento, discriminando blocos, cenas e rollback, antes de automatizar
qualquer liberação. Ver a seção "Sonda passiva de init/done". Limpeza, cinco
slots e sete hashes conferidos; usuário fechou normalmente e não há processo
restante. Recriação automática segue pendente; usar o carregamento preparado.

Contexto do procedimento anterior: validar no jogo o que a bancada já cobre. Usar o
procedimento "Teste no jogo com sessão" do README, num PC (servidor local e o
próprio jogador espelhado) e depois em LAN com dois PCs. A ordem de risco é:
skin, recriação após recarga real, vários remotos e interpolação com tráfego
real. Pose pela rede, interpolação, sessão, backup/verificação, skin e recarga
automática já estão implementados (seção "Blocos sem jogo"). As métricas de
custo/tempo de frame (M5/M8) também estão implementadas; falta medi-las no jogo.
Sinais visuais como rodas/reboque dependem de identificar as partes do modelo.
As decisões de produto da seção 3 foram aprovadas, mas não mudam essa prioridade.
Registrar separadamente modo único/meta, economia compartilhada, tradução/moeda
e hospedagem futura.
Contexto anterior — a sonda `draw-*` passou na
partida (parado, movimento, menu/recarga): o `draw` de `actors.car` com estado
6, profundidade 0, enfileira no passe do mundo esvaziado em `game+0xf3a24`.
Contrato do primeiro visual já entregue: após o `draw` original, recusar os
estados 4/9/12 e exigir profundidade 0 e path/viewport não nulos, então
`gfx_push → gfx_translate → gfx_qrotate →
psm_draw(inst,1,-1) → gfx_pop` a uma distância fixa do jogador. Instância criada
pela fila do loop, retida entre frames, liberada só após um flush posterior
completo do mundo; nada de ponteiro do jogador entre recargas.
O passe identificado pela sonda de render (`game.dll+0x800f2`) é o draw do
céu/sol (`+0x7fa90`), não todo o render. Usar `render-status`/`draw-status`
para captura pendente e manter a partida em primeiro plano.
O primeiro visual foi retido/liberado e recriado manualmente após recarga no
jogo. Eco com pose pela rede e skin com luz passaram posteriormente; manutenção/
recriação automática durante reload continua dependente da fase pré-destruição.
Lifetime, ownership, metadados,
setter PMD, descritor e instância PSM privados já têm provas registradas;
não repeti-las sem necessidade. A última sessão teve sonda/dispatcher removidos,
worker parado e encerramento normal confirmado, sem processo restante.
Reenumerar antes do próximo build. Ampliar a matriz de M1 e a robustez de M2
sem refazer a busca ampla inicial.

**Antes de começar:** ler os três documentos, conferir Git e toolchain, localizar
a instalação local e conferir hash. Não refazer a busca ampla de memória se a
cadeia documentada funcionar na versão compatível. Não usar PIDs históricos.

**Ao terminar um bloco:** atualizar checkboxes, arquivos alterados, comando/cenário
de validação, resultado, limitações, decisão de arquitetura e próxima dependência.
O remoto e a licença Apache-2.0 já foram definidos pelo proprietário. A publicação
inicial dos fontes foi autorizada sem coautor; releases de binários ainda não
foram solicitadas. Preservar esse contexto ao continuar em outra máquina.
