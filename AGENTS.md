# Orientações para continuar o projeto

Retomada Windows em 30/10: `career_entry`, `mp_career host/prepare-launch/
check-launch` e preflight de `mp_launcher session-start --career-launch` ligam
importação/identidade/manifesto à admissão do relay por ticket. Ver o primeiro
registro do ROADMAP.MD para validação e limites. Registro efêmero de 64 leases,
uma hora para novos joins/resumes, sem renovação implícita; reinício requer
ticket/arquivo novo e nova sessão do cliente. Sem ticket, as salas visuais
anteriores continuam disponíveis. Não há nova conta/autoridade econômica,
instalação/carregamento de save ou mundo sincronizado. Assistente/kit/motor
continuam pendentes. Não abrir jogos para esse bloco; G9 agrupa prova prática.
Verify Windows final: 192 x86 (146 integração/46 servidor) e 227 nativos,
releases, Clippy nos três escopos e rustfmt. Log ignorado em
target/analysis/verify-windows-20261030-career-entry-final.log. Nenhum jogo,
nova LAN, execução Linux/Proton ou atualização do kit instalado neste bloco.
Resumo geral do roadmap corrigido para reconhecer a primeira LAN real de 29/10;
não voltar a declarar ausência dessa prova a partir de notas históricas.

Retomada offline de carreira em 29/10: `mp_career` distribui uma cópia congelada
de um save-base por TCP separado do relay; manifesto/hash/convite privado,
regras declaradas Become a Millionaire/meta/sem limite, mapa/mods e importação
nova com recibo. `career_store` fornece lock SO e revisões imutáveis verificadas.
Não confundir com mundo compartilhado no motor: não carrega save/DLL, não
sincroniza horário/trânsito/física nem altera vitória original. Convite/save/
revisões são privados e ignorados; fixtures próprias para testes. Manter
entregas em blocos completos sem abrir jogos. Ver novo registro no roadmap.

Segundo bloco: `career_world` fornece ledger/checkpoints/relógio próprios;
aceite exclusivo, entrega com atestado local do host e pagamento único com
deduplicação persistente. TCP só bootstrap/consulta, sem autoridade econômica
para portadores do convite. `mp_career lab` usa dados próprios e TCP real,
sem prova do motor. `dev_env verify` agora inclui ferramentas do servidor x86
(`windows --server-tools`, manifesto separado). Não anunciar saldo/horário/
física sincronizados no jogo nem vitória original alterada. Ver limites e
resultados no novo registro do roadmap; nenhum jogo deve ser aberto por esse
incremento. Próximo: admissão/bootstrap ligados ao launcher e leitura do motor.

Leia `ROADMAP.MD` e `README.md` antes de implementar. O roadmap registra o estado
validado, as dependências, os critérios de conclusão e a próxima etapa.

O proprietário retornou ao **Windows em 28/10/2026**, após trabalhar em Fedora
Linux. Preservar os dois fluxos. Assets/servidor nativos, build cruzado Windows
x86, bancada x86 sob Proton e integração inicial da DLL na partida foram
validados no Fedora; build/bancada e uma recarga real também foram validados no
Windows em 28/10. Consultar os limites e o registro de retomada no roadmap. Não tentar
executar o toolchain host Windows nativamente no Linux nem tratar build cruzado
como teste do jogo. Coop significa LAN entre computadores distintos.

## Autonomia autorizada pelo proprietário

O proprietário autorizou grandes alterações para atingir os objetivos deste
projeto. Pode refatorar arquitetura, reorganizar módulos, criar ferramentas e
binários em Rust, mudar interfaces/protocolo, corrigir problemas e atualizar
documentação e testes sem pedir aprovação a cada decisão técnica reversível.
Os três crates atuais são o ponto de partida; uma necessidade concreta pode
justificar outra divisão, registrando a motivação e a migração.

Trabalhe em blocos completos: investigar, implementar, compilar, testar e
documentar. Continue no trabalho já autorizado após resolver uma etapa; não
termine apenas com uma proposta ou uma oferta para continuar. Planejamento e
atualizações de progresso não devem interromper a execução a cada pequeno passo.
Resuma resultados e evidências, sem narrar raciocínio interno.

O proprietário pediu em 28/10/2026 blocos maiores e menos interrupções para
testes manuais, pois trabalha em outras tarefas. Priorizar investigação offline,
fixtures, simulações e verificações automáticas; concluir e commitar conjuntos
coerentes antes de abrir o jogo. Agrupar a validação manual de várias mudanças
numa sessão em um marco significativo. Não iniciar o jogo nem pedir movimentos/
menu/load a cada incremento; solicitar intervenção antecipada somente diante
de uma falha concreta que impeça continuar ou de informação indispensável.
Registrar como pendente tudo que a bancada não comprova, sem confundir menor
frequência de teste manual com validação real já obtida.

Em 29/10/2026, o proprietário reiterou: continuar as implementações possíveis
sem o jogo e deixar a prova prática para rodada posterior agrupada. Atualizar
docs/GAME_VALIDATION.md com cada pendência entregue; não pedir para abrir o
jogo durante esses blocos nem marcar a checklist a partir de fixtures.

Pergunte quando faltar uma decisão de produto que altere substancialmente o
escopo, uma informação indisponível ou uma ação humana indispensável no jogo.
Agrupe ações manuais em um procedimento reproduzível e avance no trabalho
independente enquanto aguarda. Não invente respostas, resultados ou aprovações.
Limites da ferramenta e instruções de prioridade superior continuam aplicáveis.

## Regras do projeto

- Todo código e automação próprios devem ser Rust. Comandos de terminal para
  executar Cargo, Git e ferramentas existentes são permitidos. Não criar uma
  solução paralela em Python, C#, JavaScript ou PowerShell.
- Preservar os arquivos originais e salvamentos. Quando uma integração exigir
  alteração local, preparar cópia de segurança e restauração antes de aplicá-la.
- Nunca versionar o jogo, extrações, dumps, salvamentos, binários originais,
  credenciais ou caminhos de configuração particulares. O `.gitignore` libera
  explicitamente apenas os fontes e documentos próprios.
- O proprietário criou `rodolfoaltenrath/18wos-across-america-mp` e autorizou
  explicitamente o commit/envio inicial do projeto, sem coautor. Em 28/10/2026,
  autorizou commits automáticos ao concluir blocos validados. Preservar o
  histórico existente e não adicionar trailers `Co-authored-by`. Não inferir
  autorização para mensagens externas ou publicação de releases. Consultar o
  estado real do Git antes de novas operações.
- Tratar endereços como específicos da versão. A cadeia documentada passou em
  duas sessões locais; verificar o hash e resolver ponteiros novamente no alvo.
- Inicializar e encerrar a thread da DLL fora de `DllMain`. Validar ABI x86,
  propriedade de memória e a thread exigida pelo motor antes de chamar suas APIs.
- Executar verificações proporcionais ao risco. Usar fixtures próprias; não
  exigir assets comerciais para testes automatizados de protocolo e ferramentas.
- Atualizar o roadmap com o que foi efetivamente entregue, como foi validado,
  limitações, decisões e o próximo passo. Não marcar planejamento como concluído.

## Próximo ponto de retomada

Decisões de produto confirmadas pelo proprietário em 28/10/2026:

- Priorizar LAN; hospedagem externa é somente compatibilidade futura da
  arquitetura. Servidor independente do jogo/UI, endpoint configurável e
  separação de conexão/identidade/regras/persistência; não iniciar migração
  para VPS nem tornar infraestrutura pública requisito do MVP.
- Oferecer no mod somente a carreira baseada em `Become a Millionaire`.
  Antes de criar um save, escolher meta financeira ou checkbox `Sem limite`;
  regra acompanha o save/sessão. Sem limite desativa a condição financeira
  de vitória/encerramento, sem desativar economia e competição.
- Após o MVP visual, evoluir empresas dos jogadores, fretes exclusivos,
  economia autoritativa, frota/ranking e persistência. Provar um contrato com
  pagamento único e recuperação antes de motoristas IA/licitações/cooperativas.
- Tradução integral PT-BR e moeda R$ são futuras. Inventariar conteúdo/fontes
  e separar apresentação de valores; conversão/rebalanceamento pendentes.
  Não ampliar o mod para manter os outros quatro cenários como opções.

São requisitos/plano, não funcionalidades já entregues. Ver detalhes em M6/M7.

No Windows, usar `dev_env windows` com GCC/GNU ld da versão ativa, conforme
README. O antigo `rust-lld` x86 falhou na recuperação de panic neste host; não
reutilizar sua configuração. O helper preserva `.cargo/config.toml` local.
`dev_env verify` agora também compila o workspace nativo release: testes não
atualizam os executáveis normais. Usar servidor/assets de
`target/x86_64-pc-windows-gnu/release/` e launcher/DLL de
`target/i686-pc-windows-gnu/release/`. O preparo com eco passou novamente com
41 testes x86 e 47 nativos; não houve novo teste no jogo nessa preparação.
A última validação Windows de setter terminou com sonda/dispatcher removidos,
worker parado e processo encerrado normalmente. Não reutilizar PID/endereços
absolutos do registro.
Foi validada uma sequência dirigir/menu/carregar, com pausa transitória e
recuperação da leitura; o restante da matriz ainda está pendente.

Continuar a partir da integração inicial x86 entregue: completar a matriz de
menus/reloads e investigar criação/remoção de uma entidade visual remota (M4).
No Linux, usar `proton_host` com `runinprefix` para ferramentas no prefixo da
partida; a bancada deve executar em prefixo próprio e num único runtime via
`test_host`. No Windows, executar os binários x86 diretamente, com PID atual;
não usar `proton_host` nem copiar caminhos/prefixos do Fedora.
Não chamar exports de registro do motor sem comprovar ABI/thread/ownership.
`engine_report`, `memory_tool entities` e o dispatcher `mp_launcher engine`
estão entregues. O dispatcher comprova a thread da janela; chamadas do motor
devem passar pela fila do loop, comprovada para o ciclo privado. Consultar o
registro de validação e da saída
indeterminada no roadmap. Remover o dispatcher e descarregar antes de reconstruir
a DLL. Se a remoção falhar, manter a referência ao módulo e reabrir o processo.
`engine pump-attach` é uma sonda opcional do import `PeekMessageA`: seu uso fixa
a DLL até o fim do processo, mesmo após restaurar a entrada. Não reconstruir a
DLL enquanto esse processo existir. A ABI de diagnóstico atual é v3/664 bytes;
a ABI de configuração/rede v1 foi preservada. Ver o procedimento no README.
`phase-probe` e `private-cycle` usam a fila do loop, separada da fila de janela.
O ciclo privado usa somente `actors.car` não inicializado e destruição imediata;
não confundir essa prova de lifetime com inserção de um veículo no mundo.
Foram validados 18 ciclos reais, incluindo repetição após recarga. O próximo
bloco deve comprovar modelo/render, inicialização e
inserção/remoção; não refazer o ciclo de alocação já comprovado sem necessidade.
`memory_tool values` já decodifica sete definições herdadas na partida sem
executar callbacks; `asset_tools read` consulta definições no SCS sem extração.
O `truck_build` estava vazio no jogador carregado; os transforms declarados não
substituem a cadeia da física. Ver o contrato de armazenamento explícito do item
no roadmap antes de ampliar a integração.
`engine item-cycle` também foi entregue e validado 18 vezes no jogo, incluindo
recarga: memória fornecida pelo contêiner, referências e bytes equilibrados,
retorno ao pool. Não refazer essa prova sem necessidade. O init completo de
`actors.car` inclui criação de HUD; investigar um caminho visual sem controles/
HUD/colisão antes da próxima chamada de inicialização. O processo dessa prova
voltou a fixar a DLL; foi encerrado pelo proprietário após a limpeza.
`decals.pmd` já possui perfil explícito de metadados e lifetime privado:
`values --entity decals.pmd` leu 13 propriedades, e `item-cycle --kind pmd`
passou 17 vezes no jogo com 2700 bytes/referências equilibrados. O proprietário
dirigiu, recarregou e fechou o último processo.
`engine model-cycle --model models/ai/truck/models/truck.psm` agora foi entregue:
18 ciclos reais do setter, incluindo recarga, com bytes/referências equilibrados.
O metadado `model_name` é tipo 21; não confundir com strings genéricas tipo 14,
mesmo com callback igual. A bancada valida ABI cdecl/bool AL, limites, rejeições,
falhas e cancelamento. Não repetir essa prova sem necessidade.
Antes de init/render, resolver os slots `game.dll+0x160e20`/`+0x16114c`, que
estavam nulos na partida inclusive após recarga, e o ownership compartilhado da
base (`+0xf3fe0`/`+0xf40b0`, contador `+0x166598`). Não chamar init da fase 0 com
esses bindings nulos; investigar pré-requisitos ou outro candidato visual.
`engine_report pointers` e `bindings` agora investigam bytes/associações de
GetProcAddress sem executar a imagem; `memory_tool resources --pid PID` audita
seis slots PSM e os dois candidatos PMD sem injetar. Foram recuperados 87 bindings
da rotina `game.dll+0xce970`, usada com `lib/psm`. O provedor tem hash explícito.
A auditoria passou nas fixtures e no jogo: seis PSM corresponderam aos exports,
os dois PMD permaneceram nulos, resultados x64/x86 iguais. Correspondência
slot/export não comprova ABI/thread/ownership.
`engine info-cycle --model models/ai/truck/models/truck.psm` foi entregue e
validado 18 vezes na partida, incluindo recarga. O par cdecl
`psm_alloc_info`/`psm_info_release` libera cada descritor na mesma operação;
referência forte do item em `+0x4c`, contexto PSM em `psm.dll+0xeb84`.
A primeira carga reteve +1 no contexto; as repetições e a prova após recarga
ficaram estáveis. Não declarar todo cache/heap liberado. A fixture detecta
crescimento repetido e cancelamento sem executar trabalho atrasado.
Sonda/dispatcher foram removidos, o proprietário confirmou o fechamento e a
enumeração posterior não encontrou o jogo. Reenumerar antes de reconstruir.
`engine instance-cycle --model models/ai/truck/models/truck.psm` também passou
18 vezes, incluindo recarga: `psm_alloc_inst(info,NULL,0,1)` devolve um item
gerenciado e `psm_free` recebe esse item, não `item+8`. Dados de 26.268 bytes;
info `1→2→1`, shaders, referência/residência/acesso e retorno ao pool passaram.
O slot PSM `+0xe94c` é `ci_alloc_mem_fix` do núcleo `+0x13420`; a primeira
tentativa foi recusada antes de criar recursos por perfil incorreto, corrigido
após leitura externa. Não substituir por `mm_alloc_mem +0x13380`.
O processo da prova corrigida também foi encerrado normalmente após a limpeza.
O ciclo é privado, sem skins fornecidas/draw/física/HUD; usa shader de fallback.
`render-attach`/`render-arm`/`render-report`/`render-remove` estão implementados:
sonda passiva do import thiscall de render_pool no núcleo e binding cdecl de
path_flush no jogo. A bancada própria passou: ABI/LastError, fila `2→2→0`,
matriz ilegível, chamada estrangeira, limite de registros, conflito de ownership,
recuperação e restauração automática no stop. Mantém ABI v3/664 bytes (23–28).
Foram executados 28 testes x86 e 35 nativos, além de build/Clippy/formatação.
A sonda passou no jogo: três capturas completas (parado, após movimento/curva,
após menu/recarga), 24 registros na thread esperada, flags/foreign zerados;
duas filas `9→9→0` e `1→1→0`. Matrizes legíveis/variáveis, sem comprovar convenção
ou transform do caminhão. Retorno do flush sempre `game.dll+0x800f2`; não inferir
cobertura dos outros passes nem captura isolada do menu/carregamento.
Com a janela sem foco, o loop ficou ativo sem render; manter primeiro plano.
`engine render-status` consulta contadores/captura pendente sem rearmar.
Os dois slots foram restaurados por leitura externa; loop/dispatcher removidos,
worker parado, tarefas zeradas e hashes de originais/saves preservados.
O proprietário não notou problemas e encerrou normalmente; enumeração posterior
não encontrou o jogo. Reenumerar antes da próxima ação.
Esse retorno é o `draw` do céu/sol (`+0x7fa90`), não todo o passe 3D.
Análise estática (roadmap "Passe 3D e ponto de submissão"): caminhões são
enfileirados no `entity_t::draw` de `actors.car` (vtable `+0x12580c` →
`+0x5a6a0`) via `gfx_push/translate/rotate → psm_draw(inst,1,ARGB) → gfx_pop`;
pilha world de 16 entradas, `gfx_push` satura sem erro. `engine_report
disassemble --resolver 0xcdce0` nomeia os 151 bindings gfx.
`engine draw-attach/arm/status/report/remove` (comandos 29–34, ABI v3/664)
observa esse `draw` passivamente; bancada própria passou (30 x86, 37 nativos).
A sonda passou na partida (parado/movimento/recarga): único `this` do jogador,
chamado de `game+0xf3785`; estados alternados 6 (profundidade 0, desenha) e 12
(profundidade 1, não desenha); o passe do mundo é esvaziado em `+0xf3a24`.
A recarga recria a entidade do jogador. Slots restaurados por leitura externa,
hashes preservados; DLL fixada até o proprietário fechar o jogo.
Unidade do mundo = centímetro (caixa do truck.psm ~250×290×680).
`engine visual-show/hide/release/status` (comandos 35–39) está implementado:
instância retida e desenhada após o `draw` externo na profundidade 0, liberada
só oculta, após ≥8 flushes e com o item em `[1,1,0]`. A bancada passou
(32 x86, 38 nativos). Validado no jogo (PID histórico 10056): o caminhão
apareceu sem textura acompanhando o jogador, liberou e reapareceu após recarga;
slots restaurados e hashes preservados. A janela fora de foco não renderiza.
Entregues na bancada, sem jogo: rotação opcional no protocolo (v1 compatível),
fonte `drawn`, `visual-show --player ID|all`, interpolação (100 ms, extrapolação
≤200 ms, snap em teleporte), `engine remotes`, `session-start/status/stop/verify`
com backup SHA-256, skin `truck.shd` via `psm_set_shader`, recriação automática
após troca de entidade e servidor `--echo`. Verificar tudo com `dev_env verify`
(41 x86, 47 nativos). Próximo: teste no jogo pelo procedimento "Teste no jogo com
sessão" do README (eco num PC, depois LAN), priorizando skin e recarga real.
`psm_draw` põe protos na fila e aumenta referências de pacotes: não desenhar e
liberar a instância imediatamente sem comprovar o consumo desses recursos.
Não refazer instância/descritor/setter/lifetime sem necessidade concreta.
Distinção obrigatória: o visual sem textura seguindo o jogador foi visto no
jogo; o eco real posterior também recebeu/desenhou poses e a skin com luz foi
confirmada no reteste. Suavização na LAN e recarga automática seguem pendentes. Não afirmar
ausência de visual de teste nem multiplayer LAN pronto.

Retomada após primeiro eco real: PID histórico 15768, thread 3584. Rede/rotação
e desenho por eco funcionaram; usuário relatou atraso leve e material preto,
apesar de `skinned_parts=3`. Carregar com a instância retida FECHOU o jogo
(`0xc0000005`, pilha de liberação de heap); causa ainda indeterminada. Processo
encerrado, servidor parado, hashes de cinco originais/dois saves inalterados.
Não repetir reload com visual retido nem tratar `fault=0` como prova de segurança.
Novos caminhos: luz da cena emprestada apenas durante o draw (callback apagado
depois), `session-prepare-load` para liberar ANTES de carregar e `visual-show`
depois, `session-verify` independente do PID/DLL para conferir após saída/crash.
Fixtures verificam luz/contexto/remoção, cena inválida, preparação e verificação
após saída. O reteste real (PID histórico 5956, thread 18744) passou: usuário
confirmou textura correta dirigindo e depois de carregar/recriar explicitamente.
`session-prepare-load` provou instância ausente ANTES da carga (primeira
tentativa, sem recusas). `session-stop --verify` também passou: três slots
restaurados, worker parado, sondas removidas, tarefas zeradas e sete hashes
iguais. O usuário fechou normalmente; enumeração posterior não encontrou o
jogo, e a verificação pós-saída também retornou sete arquivos iguais. Servidor
encerrado. Não há processo do jogo com a DLL residente nessa conclusão.
Próximo bloco: observar uma fase ANTERIOR à destruição do mundo e comprovar
ABI/thread/ordem antes de automatizar a liberação/recriação. Não repetir reload
retido só porque a luz foi corrigida: a causa do crash não foi isolada.
Essa revisão passou em build release x86/nativo, 41 testes x86, 47 nativos,
Clippy x86/nativo e rustfmt. LAN com dois PCs e sessões longas seguem pendentes.

Bloco seguinte entregue na bancada: `engine transition-attach/arm/status/report/remove`
(41–46, mesma ABI v3/664) observa passivamente os virtuais init/done(phase) de
`actors.car`. Registra entrada/retorno, argumento, thread/caller, cena/física/
modelo e primeiro draw subsequente; 128 eventos por geração, flags de perda/
truncamento. Preserva forwarding, EAX e LastError. Não chama init/done adicional
nem libera recursos próprios. Sonda e visual alocado são mutuamente exclusivos.
`session-backup` copia/hasha sem injetar; parada/detach/pump-remove restauram os
hooks, e session-stop confere cinco slots. `dev_env verify`: 43 x86/48 nativos,
Clippy x86/nativo e rustfmt. A captura real SEM visual também passou: 16 eventos
na thread 6148 do processo histórico 21192, zero foreign/perdas/truncamento.
Init do novo ator (`-10,-1,0,10`) veio ANTES de destroy do antigo (`-10,0,10`),
com cenas diferentes; física antiga zerada em destroy(0), primeiro draw novo
três flushes depois. Não assumir que car::destroy(0) seja início global do load.
O relatório oferece fase assinada, preservando também os bits originais.
A fixture reproduz essa ordem com entidades distintas e fases negativas.
Os candidatos estáticos init `game+0x50f40` e done `+0x515d0` são thiscall ret4,
slots vtable `+0x5c/+0x60`; done(0) encadeia a liberação de modelos e física.
O nome done corresponde ao virtual destroy(int), separado do destrutor do
alocador. Não prova o momento global de teardown. Próximo: observar agendamento
de destroy_eblock (IAT game+0x1242b4, destino p3shared+0xc600, fila +0x3ac) e/ou
início global do load, com identidade de bloco/cena e rollback; isso é somente
candidato estático. Não inserir liberações nesse ponto sem nova prova.
Captura/backup em target/analysis/transition-20261028-01 (ignorado). Cinco slots
restaurados, dispatcher/pump removidos, worker parado, tarefas zeradas, sete
hashes iguais antes/depois da saída. Proprietário confirmou fechamento normal,
enumeração confirmou ausência do jogo. Ver seção nova do roadmap; manter
session-prepare-load/visual-show até validar a automação.

Extensão entregue: `engine transition-blocks` (47/48) acrescenta, antes do
arm, os imports init_eblock/destroy_eblock à mesma timeline. A sonda continua
passiva e incompatível com visual retido. Bool AL/ret4, LastError, aninhamento,
rollback novo sem destruir antigo e ownership foram testados com fixture
própria. Trace agora limitado a 512 eventos, com bloco/processador, dados e
contadores diretos, words crus da fila, último caminhão observado e até 16
entradas cruas de modelo. Não interpretar solicitação como destruição concluída
nem cobertura desses imports como cobertura de todo o motor. Remoção desfaz
quatro hooks e session-stop verifica sete slots. `dev_env verify`: 44 x86,
49 nativos, Clippy x86/nativo e rustfmt. A extensão passou na recarga real:
snapshot de 70 eventos, sem perdas/foreign/truncamento, 20 init de bloco/três
solicitações, profundidade até 3; novos init antes de destroy antigo, modelos
e física antigos zerados em destroy(0). Solicitações capturadas vieram depois
da troca, caller game+0xf2ff4; NÃO são o começo global do reload. No stop, status
91 registros, sete slots restaurados, filas zeradas e sete hashes iguais.
Proprietário fechou normalmente, enumeração e hashes pós-saída conferidos.
Snapshot inicial preservado; relatório integral pós-stop foi recusado porque
o jogo já havia encerrado. Não afirmar que os 91 registros foram preservados.
Seguir o procedimento transition-* do README; não liberar visual nessa fase
sem relacionar blocos/cenas/flushes e início global do carregamento.

Revisão do commit parcial `9587a15`: a extensão `transition-commands` (49/50)
agora tem captura limitada de argv[0], diagnóstico de nome ilegível/sem
terminador, cena do handler em `this+0x14c`, testes de ABI/filtro/ownership e
documentação. Cinco hooks de transição; `session-stop` verifica oito slots.
Conferidos estaticamente `game+0xd3596` e `prism3d+0xe210`. Ainda sem nova
captura no jogo: `load` é somente o comando da fixture. Não automatizar teardown
com base nesse nome. Preservar o carregamento preparado e separar os resultados
da bancada Fedora/Proton das provas Windows anteriores.

O bloco seguinte acrescenta `engine transition-report --output ARQUIVO.json`
(arquivo novo, pasta existente) e `transition_report CAPTURA [--output ANALISE]`
nativo/offline. Salvar ANTES da saída do processo; a ferramenta não recupera
registros a partir de status. JSON cru preservado com análise; pares, fases,
cenas, falhas de init e intervalos entre comandos são correlacionados sem
inferir causalidade. `structurally_complete` cobre só a estrutura do snapshot;
`automation_validated` é sempre false. Ausências/limites/perdas não comprovam
limpeza. A captura real posterior está registrada abaixo, usando o novo
procedimento do README. Não automatizar teardown nem repetir reload retido.
Este bloco passou em 51 testes nativos e 51 x86 sob Proton no Fedora, builds
release nativo/x86, Clippy nos dois alvos e rustfmt. Sem novo teste no jogo.

Captura real de comandos passou no Fedora/Proton, sem visual/rede, PID Windows
histórico 396/thread 400. `game_menu` e depois `map`, com cena antiga legível no
handler; init novo antes de done antigo, primeiro draw novo três flushes após
done(0). Snapshot inicial 48 eventos/21 pares e FINAL 61 eventos/27 pares foram
preservados integralmente; sem perdas/foreign/truncamento, todos os retornos
de bloco true. Oito slots restaurados, dispatcher/pump removidos, worker
parado, filas zeradas, seis hashes iguais (cinco originais/um save). O
proprietário confirmou conclusão/fechamento; enumeração e verificação pós-saída
passaram. A análise offline também foi repetida após a saída. Artefatos em
target/analysis/transition-command-fedora-20261028-01 (ignorado). Nenhuma DLL
residente no jogo ao concluir; reenumerar antes de reconstruir.
O ramo estático `map` confirma o candidato anterior ao tratamento da cena;
isso ainda não autoriza liberar dentro dele. Próximo: avaliar estado de
render/referências e política de recusa/recuperação antes de automatizar;
não inventar adiamento do comando nem repetir reload com visual retido.
Manter o preparo manual. Outras cargas/menus, LAN e sessões longas pendentes.

Bloco seguinte: `release_policy` compartilha as guardas com `visual-release`
e `engine visual-check` (51, ABI v3/664). Preflight somente de leitura no loop,
com thread/profundidades, contadores e idades de flush; não oculta/libera.
Corrigidos wrap de flush (`max` numérico incorreto) e de contagem de desenhos
(marcador separado). Recusas tipadas substituem busca de `retry` no texto;
preparo/parada só aguardam flush/referências, outras falhas interrompem.
`ready` vale só como snapshot, o release revalida e o relatório mantém
`automatic_load_supported=false`. Nenhuma alteração no handler de `map`.
Antes de automatizar, comprovar contexto/ordem e contrato de adiamento ou
recusa da carga. Não esperar render dentro do handler nem chamar recursivamente.
Manter o preparo explícito e não repetir reload com visual retido.
Este bloco passou em 55 testes nativos/55 x86 sob Proton, builds release
nativo/x86, Clippy nos dois alvos e rustfmt no Fedora. Sem novo teste no jogo
nesse commit. Antes dos builds, a enumeração não encontrou o jogo.

O mesmo preflight passou depois no jogo Fedora/Proton (PID Windows histórico
420/thread 424): consulta sem instância, recusa por visível apesar de `[1,1,0]`,
prontidão após ocultar, uma consulta/uma tentativa no prepare-load e confirmação
de item zero depois. Sempre no loop, profundidades 1/0/0/0, sem rede nem sonda
de transição. Oito slots restaurados, filas zeradas e seis hashes iguais antes
e depois da saída. Proprietário informou saída, enumeração final confirmou
ausência do jogo. Nenhuma DLL residente; reenumerar antes de novo build.
Não houve recarga retida nem prova de liberação dentro de `map`.
Pista estática: `cmd_execute` usa argc/argv globais apontando para sua pilha;
reentrância sobrescreve os globais. `cmd_add` copia no máximo 127 bytes em
buffer de 100 entradas; retorno de sucesso não comprovado. Não chamar nem
usar para replay ainda. Próximo: comprovar consumidor/ordem/ownership da fila
e descritor do callback de `map`, limites e recuperação antes de impedir carga.
Ver o novo registro e os artefatos ignorados release-check-fedora-20261028-01.

Bloco offline seguinte (sem abrir jogo/interromper o proprietário): auditor
`memory_tool commands --pid PID` com perfil/hash, registro limitado a 1024,
ciclos/releitura, descritor `map` em game+0x1573c4 → thunk +0xd2e90.
`cmd_add` não tem consumidor de execução adiada comprovado: não usá-lo como
fila por hipótese. A auditoria não lê argv nem argumentos de saves.
`deferred_load` entrega fila própria de uma carga, argumentos copiados e
redigidos nos logs, tickets por instância/operação, timeout/cancelamento,
confirmações e recuperação explícita. Sem ligação com o jogo/novos hooks.
`load_lab` roda seis cenários por iteração; 10.000 iterações nativas passaram
(60.000 cenários simulados). Fixtures x86 usam argv/callback próprios, sem
assets comerciais. `automatic_load_supported=false` e `game_tested=false`.
Próximo: adaptador do descritor/argv na bancada, com ownership/restauração e
conflitos; depois agrupar as verificações numa sessão real em marco maior.
Não pedir ao proprietário para abrir o jogo a cada incremento. Consultar o
registro novo no roadmap; a auditoria/controle novos ainda não passaram no jogo.
Validação desse bloco no Fedora: 68 testes nativos/70 x86 sob Proton, builds
release, Clippy nativo/x86 e rustfmt passaram. `load_lab` release passou 60.000
cenários por alvo, nativo e Windows x86, com JSON idêntico. Artefatos batch-*
ignorados; jogo não foi aberto e não houve teste manual nesse bloco.

Adaptador seguinte entregue somente no host próprio: `deferred_load::adapter`
tem descritor repr(C) de 16 bytes no x86, callback +4, instalação/restauração
por CAS sem sobrescrever outro dono. Binding argc/argv próprio, revisionado,
com array/CStrings retidos durante a chamada, exclusão de nesting e RAII.
Revalida contexto/mundo/prazo/token/identidade dos argumentos imediatamente
antes do despacho original; cancelamento/expiração não executam a ação antiga.
Conflito após chamada vira Unknown, sem replay/recriação. Words externos são
preservados; argumentos continuam retidos até prova explícita de ausência de
leitores. Descartar o host sem essa prova mantém a memória, não a libera.
`load_lab --adapter --iterations 10000` exercita oito cenários, com callbacks
cdecl próprios reais e recursos/mundos simulados. Não aceita endereços reais,
não instala hook no jogo e não amplia a ABI v3/664. Não confundir revisão dos
words serializados da fixture com quiescência dos globais do núcleo; CAS
também não resolve ABA do descritor real. Preparação manual antes de carregar
continua obrigatória. Próximo marco é captura passiva no descritor perfilado,
relacionada ao lifecycle, antes de habilitar interceptação ativa; agrupar as
verificações reais, sem abrir jogo a cada incremento. Ver novo bloco no roadmap.
Validação desse adaptador no Fedora: 77 testes nativos/79 x86 sob Proton,
builds release, Clippy nos dois alvos e rustfmt passaram. O lote de 80.000
cenários/40.000 callbacks próprios passou nativo e Windows x86, JSON idêntico.
Recursos visuais/epochs são simulados; não é teste do callback comercial nem
recarga real. Artefatos adapter-* ignorados, jogo não foi iniciado neste bloco.

Bloco seguinte entregue na bancada: `engine transition-map` (52/53, ABI v3/664),
sexto hook passivo em `descritor map+4`, instalado após `transition-commands`
e antes do arm. Thunk `game+0xd2e90` conferido: cdecl sem argumentos, lê o
handler em `game+0x160df0` e salta para `+0xd3570`; EAX de retorno é só
diagnóstico. Registra argc/argv/handler e somente `argv[0]`; `argv[1..]`
nunca é lido. Chamadas em curso bloqueiam remoção/arm; `session-stop` verifica
nove slots. `transition_analysis` gera `map_calls`, sem autorizar automação.
81 testes nativos/84 x86 sob Proton, builds release, Clippy e rustfmt passaram.
Captura real passou (Fedora/Proton, PID histórico 428/thread 432, sem visual):
auditoria `commands` com 33 comandos, `map` conforme o perfil; o thunk
`+0xd2e90` atende oito comandos. Snapshot de 68 eventos íntegros: novos
blocos e init `-10,-1,0,10` do caminhão novo DENTRO de `map`, zero flushes,
done antigo `-10,0,10` logo após o retorno. Nove slots e sete hashes conferidos
antes e depois da saída; nenhuma DLL residente. Liberar dentro de `map` está
descartado; próximo passo é adiar `map` com `deferred_load` na bancada, com
fallback para encaminhar. Não repetir reload retido. Artefatos em
target/analysis/transition-map-fedora-20261028-01 (ignorado).

Gate ativo de `map` entregue na bancada (commit `fe52021`, comandos 54–57, ABI
v3/664): com o visual alocado, o hook em `descritor map+4` copia argc/argv,
oculta e NÃO executa; o pump libera pela política, publica argc/argv próprios
em `prism3d+0x778f0/+0x778f4` e chama o original uma vez (contrato estático de
`cmd_execute` `+0xe010..0xe01c`), depois recria o visual quando o draw vê um
ator novo. Sem visual, encaminha. `session-start` instala por padrão
(`--no-load-gate`, `--load-timeout`); stop/prepare aguardam quiescência.
`map_gate_workflow` cobre onze cenários; 84 nativos/88 x86, Clippy e rustfmt.
NÃO testado no jogo: `automatic_load_supported=false`. Só `map` passa pelo gate
(não quit/exit/novo jogo); manter `session-stop` antes de fechar. Próximo:
sessão real agrupada com eco (roadmap "Gate ativo de `map`"); se falhar, voltar
a `--no-load-gate` + `session-prepare-load`.
VALIDADO no jogo (Fedora/Proton, PID histórico 396/thread 400, eco 127.0.0.1):
duas cargas de `test_mp` pelo menu com o visual visível, sem prepare-load:
`deferred/replays/recreated=2`, argc/argv restaurados, `dropped=0`, sem crash;
proprietário viu o caminhão espelhado voltar com textura. Stop: nove slots,
sete hashes; jogo fechado, `session-verify` igual, servidor parado. Próximo:
`quit`/`exit` pelo mesmo mecanismo e LAN com dois PCs. Janela sem foco congela
render/rede (não é falha do gate).
Gate estendido (commit `d98efb6`) a `preview`/`edit`/`exit`/`menu`/`quit`: o
mesmo handler apaga a cena nesses ramos (estático; `game_menu`/`pause` não).
Classifica por argv[0]; sem mundo de jogador, o job conclui no retorno e o visual é
recriado no próximo ator distinto (`restore_pending`); `quit` nunca restaura.
87 nativos/91 x86, Clippy e rustfmt; cenário `exit`→menu→`map` na fixture.
Sem teste no jogo desses cinco. Próximo: sessão real agrupada (exit, carga do
menu principal, quit com visual) e depois LAN com dois PCs.

Revisão Windows de 29/10/2026 (base `26ece45`): corrigidos journal/rollback do
gate. Os slots são registrados antes da escrita; callback e proteção de página
têm recuperação separada. Se o rollback falhar, o gate continua instalado para
permitir nova remoção e recusar reinstalação. Nunca sobrescrever um callback de
outro dono. Instalação e `session-stop` agora resolvem os seis descritores com
a mesma auditoria; o stop confere 14 slots. Os nove slots dos registros antigos
são históricos. Novas fixtures cobrem falhas parciais e uma página read-only real
Win32 x86 com seis palavras; não executam callbacks do jogo.

Entregue `network_lab` (M8): três cenários com relógio virtual, protocolo e
interpolação de produção, perda/duplicação/atraso/jitter/reordenação, fila limitada
e interrupção/recuperação. O filtro de sequência é simulado; não chama worker,
sockets ou motor. `game_tested=false`/`lan_tested=false`. O lote de 36.000 pacotes
por cenário passou em release Windows x64 e x86 (108.000 gerados por alvo), com
JSON idêntico. Sem nova execução Linux ou partida neste bloco. README contém
comando e limites; artefatos `network-lab-windows-20261029-*` ignorados.

`dev_env verify` final passou: 101 testes x86 e 101 nativos Windows x64, builds
release, Clippy nos dois alvos e rustfmt. Não comparar esse total nativo com o
do Fedora sem considerar testes condicionais Windows. Jogo não foi iniciado;
nenhuma nova DLL injetada. Reenumerar antes de novo build/uso. Próximo marco de
integração preservado: sessão agrupada exit/menu/map/quit com eco e, depois,
LAN entre dois PCs. Sem prova nova dos cinco comandos nem de LAN neste bloco.

Continuação de 29/10/2026 após `7ae5f3e`: extraído `network_conditions`, agora
compartilhado por `network_lab` e novo `udp_proxy` nativo. Proxy usa dois sockets,
um cliente fixado após primeiro pacote v1 válido, filas/tamanho/trabalho/duração
limitados, perdas/duplicação/atraso/jitter por sentido e relatório sem conteúdo de
pacotes. Interrupção programada conta após o primeiro cliente válido; prazo total
desde início do proxy. Ao terminar descarta pendentes; reiniciar proxy quando o
worker mudar a porta de origem. Erros transitórios de porta inacessível não o
encerram. Seed reproduz sorteios, não o agendamento real dos sockets.

`udp_proxy_workflow` passou: DLL x86 real em fixture Rust própria, UDP degradado,
sequência antiga com pose errada somente após testemunhar uma pose mais nova,
versão inválida, pacotes grandes, interrupção ≥1200 ms e retomada; worker continua
enviando, histórico ≤16 amostras, stop/join e descarga. Não usa hooks do motor nem
prova que um visual foi ocultado no jogo. Testes próprios de sockets cobrem outro
remetente, tamanho e servidor temporariamente ausente.

Também passou servidor Tokio com eco + proxy nativo + `client_probe`/DLL x86
simulada, em loopback: oito segundos, 476 envios/121 amostras novas aceitas,
contador parado durante a interrupção de 1,5 s e depois recuperado, exit 0.
Proxy: 90/75 descartes na interrupção, filas máximas 8/7 e zero pendentes ao
terminar. Relatórios `udp-proxy-final-*20261029*` ignorados. Fluxo virtual antigo
repetido em x64/x86 com 36.000 pacotes por cenário: JSON idêntico ao anterior,
SHA-256 c63d182b8b980832ccc60abbda6045077512255dbccbc87111b9046ad73ca678.

`dev_env verify` final: 108 testes x86 e 107 nativos Windows x64, builds release,
Clippy nos dois alvos e rustfmt. Ver log ignorado
`target/analysis/verify-windows-20261029-udp-proxy-final.log`. Sem nova execução
Linux, jogo ou LAN entre PCs; validação do fluxo Linux ainda pendente. Servidores
auxiliares encerrados, proxy terminou pelo prazo e jogo ausente na enumeração.
Não reconstruir com DLL residente: reenumerar sempre. Próximo marco de integração
preservado: sessão agrupada exit/menu/map/quit com eco, depois LAN. Não confundir
esses testes de sockets com negociação/heartbeat/reconexão de M3 ou jogo validado.

Proprietário pediu em 29/10/2026 continuar tudo que puder ser implementado sem
intervenção prática dele. Não solicitar jogo/movimentos nesta rodada: concluir
blocos offline, testes automáticos, documentação e commits sem coautor.

M3 v2 entregue optativamente após `d4972ff`: consultar docs/NETWORK_PROTOCOL.md.
Servidor `--room` cria uma sala; v1 segue padrão. Hub/Client portáveis, handshake
idempotente com recusa tipada, identidade local persistente, credenciais/generação
por conexão, retomada/reinício de sequência, heartbeat mesmo sem coordenadas,
timeout, rosters revisionados periódicos, leave confirmado/repetível, RTT/offset
diagnóstico e limites antes do parsing. Janela de tráfego não zera ao retomar.
Identidade não é conta autenticada; ID numérico/cache não persiste no servidor.
Não aplicar ainda o offset ao render ou tratar tick de rede como física.

DLL/launcher: START_SESSION=4 e SESSION_STATUS=5 na Control v1 existente, sem
mudar layout. JSON em message; session-start/connect --room --identity-file,
network_session em session-status. Sem flags: v1. API/deps geram arquivos de
identidade próprios, sempre locais/ignorados. Tokens só no wire, nunca no status.
ClientConfig/EngineControl e os hooks do motor não foram ampliados.

Validação Windows: 116 testes x86/125 nativos x64, release, Clippy e rustfmt.
Fixtures cobrem oito seeds com perda/duplicação/reordenação e reinício da sala.
DLL real em fixture passou: 2300 ms sem coordenadas (timeout 2000) com heartbeat,
geração remota nova/histórico zerado, servidor reiniciado, retorno de coordenadas,
remoção por roster e leave confirmado/descarga. Servidor Tokio com dois clientes
passou. Smoke release: A via proxy com interrupção de 2500 ms, duas retomadas,
331 poses e leave confirmado; B recebeu 278 poses, mudança de geração e leave.
Perfil errado recusado; servidor acabou sem peers. Artefatos session-v2-* em
target/analysis, ignorados. Nenhum auxiliar ficou aberto. Sem jogo/Linux/LAN
entre PCs ou prova nova dos cinco comandos. Autenticação/economia, relógio de
render, modelo/velocidade e sessões longas continuam pendentes.

M5/M8 offline: regressão de rajada UDP reproduziu extrapolação de 20.000.100 cm
a partir de 100 cm recebidos em 1 microssegundo. Corrigido denominador mínimo
de 1/60 s (mesmo critério de teleporte), timestamp igual substitui a última pose,
rejeição de amostras inválidas e intermediários f64/fallback finito. Mantidos
100 ms de atraso e 200 ms de previsão, sem aplicar offset de rede ao render.
`dev_env verify` Windows passou: 119 x86/128 nativos, release/Clippy/rustfmt.
Sem jogo ou LAN. Executou-se uma cópia do helper em target/analysis para não
bloquear sua própria atualização; o bloco seguinte corrigiu esse fluxo.

M0/M7/M8 offline: `asset_tools audit-source RAIZ --scope worktree|index|head`,
`source-bundle RAIZ --output NOVO.zip` e `verify-source-bundle ARQUIVO` entregues.
Lista de permissão independente do ignore; índice/HEAD leem blobs exatos. ZIP
somente dos fontes commitados no HEAD, com manifesto/tamanhos/SHA-256; ignora
edições posteriores e não inclui .git/jogo/extrações/saves/config/binários.
Não afirmar autoria/assinatura/ausência de texto sensível ou auditoria de todo
o histórico. Clone e rebuild seguem preferidos. Auditoria de binários/assets,
instalador e avisos de dependências continuam pendentes.

`dev_env verify` agora audita o worktree. No Windows, estaciona só o próprio
executável em target/dev-env-running para permitir rebuild; restaura em falha
pré-link sem sobrescrever saída nova, limpa cópias antigas na próxima execução.
Fluxo normal passou com 119 testes x86/136 nativos, release, Clippy e rustfmt;
configuração Cargo local manteve hash. Fixtures cobrem fonte forçada/disfarçada,
índice diferente do disco, link/case collision, limites, travessia, tamper e
ZIP determinístico. Auditoria local: 120 fontes, ~1,56 MB. Artefatos source-*
em target/analysis ignorados. Nenhum teste novo no jogo/Linux/LAN entre PCs.
Não solicitar sessão manual a cada incremento; agrupar a matriz pendente depois.

Sessão prática agrupada autorizada e concluída em 29/10/2026, revisão dd24929:
Windows, PID histórico 15204/thread 19152, v2 sala validacao/eco loopback, seis
descritores perfilados, backup de dois saves + hashes de cinco originais.
Proprietário confirmou caminhão com textura/movimento/curvas e atraso leve.
Servidor encerrado e reiniciado: snapshot reconnecting/remotes vazios; depois
outra sessão de runtime, accepted=2/reconnects=1 e retorno do draw. Ele NÃO
percebeu desaparecimento; não marcar visibilidade em desconexão como provada.
Última submissão ficou 17036 flushes atrás no snapshot da interrupção.

Duas cargas reais pelo gate de map: deferred/replays/recreated/arguments_restored
=2, sem argumentos retidos/dropped/foreign/fallback/busy; idle ao final. Captura
execute teve release_confirmed=true/item=0. Usuário navegou pelo menu e aprovou
as duas voltas do caminhão; só duas chamadas map chegaram aos hooks. Não inferir
prova de exit/menu/preview/edit/quit nem testar quit ativo sem captura apropriada.
Conexão permaneceu aceita nas cargas. Atraso leve continua em aberto; buffer
permanece 100 ms, não foi ajustado cegamente durante a prova.

Stop passou: uma consulta/uma tentativa, sem recusas, 14 slots restaurados e
sete hashes iguais. Pós-stop: item zero, gate ausente, closed/leave_confirmed,
worker parado, dispatcher inativo/pump removido, filas/inflight zero. Servidores
encerrados e relatórios preservados. Usuário fechou normalmente; enumeração
confirmou ausência do jogo e verificação pós-saída sete arquivos iguais.
Nenhum processo residente ao concluir; reenumerar, não reutilizar PID/endereço.
Artefatos ignorados: target/analysis/session-v2-game-20261029-01/. Só documentação
mudou; usaram-se os binários já aprovados em 119 x86/136 nativos, sem rebuild
com DLL residente ou alegação de nova suíte após a partida. README/roadmap atuais
distinguem o gate validado da detecção tardia que causou o crash histórico.
Próximo marco: dois computadores na LAN, atraso/jitter, vários remotos, saída de
pares e observação de desaparecimento. Outros comandos/transições e sessões
longas pendentes. Preservar automatic_load_supported=false, v1 padrão e fallback.

Bloco offline seguinte, após o proprietário informar que não conseguiu abrir o
jogo no Linux e não podia testar dois PCs: suavização configurável e métricas
de tempo. Nenhuma nova partida foi solicitada/aberta. `presentation` consulta ou
ajusta --delay-ms/--extrapolation-ms (0..200); session-start recebe
--interpolation-ms/--extrapolation-ms. Padrões 100/200 mantidos, sem alegar que o
atraso leve foi corrigido no jogo. Configuração atômica; modos de avaliação,
stale/sem pose/contenção em presentation, cumulativos desde a carga da DLL.
Control v1 preservada (1536), novos comandos 6/7; Engine v3/664 e hooks intactos.

`network-timing`/network_timing (comando 8) registra por conexão aceita:
RTT máximo/amostras/variação e variação de intervalo de chegada vs server_ms,
EMA inteira 1/16 em us, gaps/passos de tick e não monotônicos. Mesmo tick não
troca a base; regressões contadas sem contaminá-la; novo aceite zera e leave
preserva. V1 retorna null. Nada de credenciais ou campos novos no wire. Lacunas
e old NÃO estimam perda; agendamento/polling também afetam os números. Offset
continua diagnóstico e não alimenta o render. session_probe exporta timing.

network_lab schema 2 mede modos/erro contra trajetória própria, com linear e
stop-turn (duas paradas e mudanças de direção contínuas em 8 s). Lote: 2 caminhos
x 3 orçamentos x 2 alvos x 3 cenários x 36.000 poses = 1.296.000 geradas; seis
pares JSON idênticos x86/x64. Menor atraso médio pode aumentar o erro máximo;
nenhum novo padrão recomendado. Não comparar seus hashes com schema 1 histórico.

dev_env verify passou Windows: 127 testes x86/148 nativos, release/Clippy/rustfmt.
Fixture com DLL real validou ajuste, retenção/stale/sem pose e timing durante
heartbeat sem coordenadas. Dois clientes release reais/servidor Tokio e proxy
passaram em loopback: ambos conectados, leaves confirmados, servidor sem peers
ao terminar e auxiliares encerrados. Logs/lotes/identidades em
target/analysis/offline-presentation-20261029-01/ (ignorado); log da suíte em
target/analysis/verify-windows-20261029-presentation-timing.log. Nenhum jogo,
teste Linux ou LAN novo. Reenumerar antes de próximo build.

Próximo marco prático continua LAN/visibilidade e outros comandos de teardown,
quando houver ambiente disponível; trabalho offline não substitui essa prova.
A cópia local preparada para o pendrive permanece na revisão anterior; novos
comandos exigem transferir launcher/DLL atualizados juntos, não presumir que
uma compilação atualize automaticamente aquela cópia.

Bloco offline seguinte (M7/M8): `asset_tools mod-example/mod-build/mod-verify/
mod-stage/mod-unstage` entregues, sem jogo/DLL/rede novos. Projeto schema 1
declara source relativo em inputs/owned ou inputs/local, caminho interno,
origin owned/derived_local e depends_on (somente referências internas). Pacote
imutável de dois arquivos: overlay.scs Stored canônico e manifest.json com
SHA-256/tamanhos/origem/dependências. Build só reutiliza saída exatamente igual,
sem reescrever; mudanças exigem destino novo. Sem cache de conversão ou codecs
novos. O modo --distributable recusa derived_local; autoria não é reconhecida
automaticamente. Extensões data-only e assinaturas comuns PE/ELF/ZIP filtradas;
não tratar isso como auditoria completa ou assinatura.

Stage usa pasta nova; unstage verifica tudo, cria/confere backup novo, revalida
e remove só os dois arquivos esperados. Extras/adulterações recusados; falha
pode deixar parcial, preservando backup para stage em outro destino novo.
Sem exclusão recursiva, sem sobrescrita e sem transação contra energia/escritores
concorrentes. Links/junctions recusados nos ancestrais; fixture de symlink
Windows não pôde criar link por erro 1314, portanto esse ramo não foi provado
neste host. Fixture Unix aguardando execução Linux. Bytes de PNG/alpha passaram.

dev_env verify passou: 127 x86 existentes/165 nativos, builds release, Clippy e
rustfmt. São 17 testes novos nativos; não afirmar que a CLI de assets foi testada
x86 ou Linux. Teste real de arquivo bloqueado confirmou backup/restauração após
remoção parcial. CLI release nativa passou todo o ciclo e reutilização; duas
raízes/backup/restauração produziram hashes iguais. Artefatos ignorados em
target/analysis/mod-package-windows-20261029-01/ e log
target/analysis/verify-windows-20261029-mod-package.log.
session-verify pós-bloco: sete originais/saves iguais, jogo ausente e configuração
local do Cargo preservada. Não reaproveitar PIDs/endereço de sessões históricas.

IMPORTANTE: aa/init.cfg local monta SCS explicitamente. Stage NÃO ativa mod no
jogo, não instala hooks nem edita a configuração. Prioridade de montagem e
instalador integrado permanecem pendentes, assim como reconstrução comercial
local/transformações incrementais. Não publicar SCS reempacotado do jogo nem
marcar M7 inteiro como entregue. Continuar o trabalho offline quando útil;
LAN e demais comandos de teardown seguem dependentes de marco prático agrupado.

Receitas locais/cache entregues no bloco seguinte: asset_tools
mod-recipe-example/mod-source-hash/mod-recipe-build. Receita schema 1 pinna
arquivo e entrada por SHA-256; owned_file em inputs/owned, local_file em
inputs/local, local_archive ligado por --archive sem paths privados no manifesto.
Até 16 originais de 2 GiB, hash em fluxo/entrada de até 16 MiB; somente a entrada
escolhida é descompactada. Operações copy/PNG, flip Y e redução proporcional.
Cache separado de inputs/saída, chave por conteúdo/extensão/operação/revisão/
Cargo.lock; cabeçalho/tamanho/hash conferidos. Mesmo com hit, originais são
revalidados. Corrupção aborta, sem reparo ou sobrescrita silenciosa; cache é
local de confiança, não autenticado e pode conter conteúdo derivado privado.

Pacote schema 2 registra proveniência/revisão/lock por entrada; schema 1 antigo
continua válido sem mudar bytes. Origem local não é promovida a owned por
converter/usar cache; --distributable recusa derivados. Autoria ainda é declaração,
não inferência. Ajustar REVISION se mudar implementação das transformações;
troca de dependências do lock invalida chaves automaticamente. Texturas têm
limites de dimensão/pixels e orçamento best effort do decoder; ICC/EXIF/mipmaps
não são preservados pelo contrato. Fixtures TGA/BMP/PNG 16-bit/RGBA passaram.

14 testes novos nativos e CLI release própria: cache frio 0/2, quente 2/0,
pacotes idênticos, stage/backup/restauração schema 2 e compatibilidade com pacote
schema 1 anterior. Fingerprint de uma entrada JPEG do base.scs também passou
sem extração; não confundir isso com reconstrução/ativação na partida. Artefatos
em target/analysis/mod-recipes-windows-20261029-01/; ver resultado final da suíte
no roadmap e verify-windows-20261029-mod-recipes.log. Sem jogo/Linux/CLI x86 novo.
Verificação final: dev_env verify passou 127 x86 existentes/179 nativos,
release/Clippy nos dois alvos/rustfmt; 14 novos testes nativos. Fingerprint
prevalida arquivo regular antes de abrir para recusar FIFO sem bloquear no Unix;
fixture Unix adicional com mkfifo ainda não executada aqui. Ramo de criação
de symlink Windows continua sem privilégio, não contar como prova desse ramo.
Reuse release final: 2 hits/0 misses/reused=true; sete originais/saves iguais,
jogo ausente e configuração Cargo preservada. Sem mudança no wire ou nos hooks.

docs/GAME_VALIDATION.md agora concentra G1–G8 e deve receber as próximas
pendências; nenhum teste futuro foi marcado como passado. Gate dos demais
comandos e ativador de assets precisam de preparo técnico antes da ação humana.
Foi ampliada a allowlist exclusivamente para esse documento próprio. Ao mudar
essa allowlist, reconstruir dev_env com windows --native antes de verify se o
helper antigo recusar o documento: sua auditoria está embutida no binário.

Montagem/prioridade/ativador no jogo continuam pendentes. Não editar init.cfg,
não iniciar jogo nem pedir validação manual durante o trabalho offline já
autorizado; manter automatic_load_supported=false e as proteções existentes.

Bloco seguinte autorizado: endurance UDP e diagnóstico estático, sem abrir o
jogo. `session_soak` (multiplayer_server) usa sala Tokio e Client de produção,
2..64 clientes/sockets reais em loopback no mesmo processo, poses sintéticas,
duração até 1 h, churn de saída/queda/retomada/identidade nova e reinício normal
ou aborto da tarefa. Não mata processo externo nem usa identidades pessoais.
Exige malha observada por período, estabilização final, leaves confirmados,
servidor sem peers ativos e limites; parâmetros inválidos/overflow recusados.
Períodos abortados não têm métricas finais do servidor (null). Primeira malha é
timestamp global e pode ser adiada por churn concorrente, sem medir recuperação
isolada. Não confundir falta de observação com causa já isolada de falha.

Release Windows: 16 clientes/300 s, seis abortos com pausa de 3 s, 3.474.812
poses recebidas/250 aceites; 16 leaves confirmados/zero ativos finais. 32/60 s,
três reinícios normais de 500 ms, 2.533.024 poses/179 aceites; 32 leaves/zero
ativos. Ambos sem issues e sem frames inválidos/recusas acumulados nos clientes.
Cache final reteve 16/32 membros; não declarar heap/cache inteiro liberado.
CPU do processo 24,57%/67,09% de um núcleo; RSS amostrada máxima
7.655.424/6.807.552 bytes. Inclui servidor/clientes/driver/runtime, com outros
builds/testes no host; não é benchmark isolado, FPS ou prova de ausência de leak.
Backend Windows consulta só o próprio processo; Linux procfs/sysconf tem fixture,
sem execução Linux nova. libc/windows-sys viraram dependências diretas opcionais
da feature server, nas versões já fixadas; DLL x86 continua sem essa feature.

`mp_doctor` (client_dll, nativo recomendado) reutiliza profile e confere cinco
originais, DLL/launcher x86/exports e servidor nativo opcional por leitura em
disco. Não abre processo/injeta/carrega DLL nem instala DirectX/.NET. Game root
via --game/AA_GAME_DIR/ancestrais do cwd, pastas de binários independentes.
Relatório sem paths privados por padrão, --include-paths opt-in e --output novo.
PE32/PE64 limitados, ELF só cabeçalho, arquivos até 64 MiB; prefixo apenas
drive_c/user.reg/system.reg. Presença de imports é informação, não resolução de
loader/WinSxS/Wine. Runtime, ABI ao vivo, revisão coincidente do par e mapa/mods
continuam false/não verificados mesmo com static_checks_passed=true.

Doctor x86 real passou sete arquivos; x64 passou oito incluindo servidor. Pasta
errada de clientes x64, prefixo inexistente e overwrite de relatório recusados;
o relatório original permaneceu idêntico e sem paths privados. Sete novos testes
de diagnóstico x86/nativos, cinco novos do servidor nativo. dev_env verify:
134 x86/191 nativos, release x86/nativo, Clippy ambos/rustfmt limpos. Ver
target/analysis/verify-windows-20261029-soak-doctor.log e artefatos privados em
target/analysis/soak-windows-20261029-01/. Sete originais/saves iguais e config
Cargo preservada; game/soak/doctor encerrados, sem PID histórico reutilizável.

README/roadmap e preparação de docs/GAME_VALIDATION.md atualizados. Não há nova
partida, LAN, teste Proton/Linux ou prova de render/lifecycle/FPS. Roteiro G5/G6
permanece pendente; doctor é preflight estático, não preparo automático de
ambiente. Kit de binários próprios/CI e fluxo de instalação continuam futuros.

Parada solicitada pelo proprietário para desligar/trocar de computador em
29/10/2026: consultar primeiro o novo "Ponto de parada para troca de computador"
no início de ROADMAP.MD. A última correção do soak (overflow de prazo, com teste)
também passou no verify completo: 134 x86/191 nativos, release, Clippy e fmt.
Não continuar a implementação nesta parada. Bloco preservado por commit local
sem coautor; é necessário transferir o commit para retomar em outra máquina.
Artefatos ignorados não viajam pelo Git; não confundir com prova Linux/LAN/jogo.

Retomada autorizada em 29/10 no Fedora: a pausa histórica acima foi revogada
pelo pedido de continuar. Main local divergente reconciliada com origin/main
por merge `2901603`, preservando `69e71e7` e os dez commits Windows. Backup em
`backup/main-before-sync-20261029`. Worktree antigo de claude/e2e-automation
(ancestral) com três arquivos alterados e e2e_harness ignorado foi preservado;
não trocar o código atual por esse experimento nem apagá-lo silenciosamente.

`dev_env verify` agora também tem fluxo Linux: flags --steam/--compat/--proton/
--runtime, prefixo já inicializado exclusivamente de bancada sob target com
canonicalização, lock exclusivo, guarda de prism3d.exe em /proc, release
nativo/x86, bancada x86 num runtime via proton_host/test_host, testes nativos,
Clippy ambos e fmt. Encerra apenas o wineserver do prefixo de teste validado;
nunca apontar ao compatdata da partida. Confere config Cargo sem reescrevê-la.
Windows mantém verify sem flags Proton e seu parking do executável em uso.

Último bloco ec5d993 revisado e exercitado no Linux: doctor passou oito arquivos,
servidor ELF/prefixo e recusas controladas, sem paths privados. Continua sem
prova de loader, par da mesma revisão, assets, runtime do jogo ou LAN.
Soak inicial Fedora 16/60 s falhou epoch_1_no_full_mesh apesar da recuperação
final: quedas agendadas em grupo podiam impedir a observação exigida pelo
próprio driver. Preservado o relatório falho. Agora churn aguarda malha completa,
alterna modos entre peers e registra churn_wait_ticks; intervalo é mínimo.
Calendário de reinício e exigências por época/limpeza final mantidos.
Reteste 16/60 s com três abortos e 32/30 s com dois reinícios normais passou,
todos os leaves confirmados, zero ativos finais, sem issues/invalid/refusal.
Não declarar zero ativos nos reinícios intermediários nem métricas de épocas
abortadas; CPU/RSS são do processo inteiro e houve cargas concorrentes.

Verify final Fedora: 134 x86 sob Proton/189 nativos, releases, Clippy e rustfmt
passaram; ramo Windows do helper conferido por build check cruzado. Logs em
target/analysis/verify-fedora-20261029-final.log e doctor-fedora-*/soak-fedora-*.
Nenhuma partida ou ação humana neste bloco. G5/G6 continuam pendentes, assim
como teardown real dos demais comandos. Próximo incremento offline pode
preparar kit próprio/identificação de builds, sem publicar release; manter
docs/GAME_VALIDATION.md para a rodada prática agrupada.

Em 29/10 o proprietário confirmou um segundo PC Windows na mesma LAN e pediu
uma pasta em Downloads com os arquivos para a raiz do jogo. Isso autoriza o
kit binário local, não uma release pública. `mp_kit` empacota somente cinco
binários próprios x86 e configuração/instruções/licença/manifesto SHA-256;
`mp_lan` confere perfil/integridade e compõe backup/início/status/parada via
mp_launcher. Identidade, backup, logs e parada pendente ficam no LOCALAPPDATA
do destinatário, nunca no kit nem Git. Room v2 igual, identidades diferentes,
offset zero e servidor no Fedora sem --echo. Conferir o registro M9 novo e
G5/G8; build/fixtures/checagem estática não aprovam LAN nem o início/parada
do assistente na partida do destinatário. Não abrir o jogo local neste preparo.
Verify deste bloco: 136 x86/191 nativos, releases, Clippy e fmt; doctor nativo e
check-only do assistente sob Proton passaram. Menu via Proton recebeu EOF antes
de qualquer opção; interativo/início/parada do assistente seguem para G5/G8.

Rodada posterior de 29/10: primeira LAN Windows↔Fedora/Proton REAL passou, sem
eco/offset artificial. Proprietário confirmou presença mútua, sumiço ao perder
foco e recuperação ao voltar. Logs Windows de início/status/stop foram recebidos
e cruzados com Fedora/servidor; consultar o registro completo de métricas em
docs/GAME_VALIDATION.md e o novo registro M3/M4/M5/M9 do roadmap. Preservar essa
prova, sem repetir percursos só para recuperar números. Pasta privada da rodada
target/analysis/lan-two-pc-20261029-01; contém identidades/backups fora do Git.
Exportar somente a lista de diagnósticos, nunca o diretório completo recebido.
Windows usa Documents redirecionado OneDrive; caminho real resolveu o preparo.

RTT Windows 16 mínimo/17 atual/34 máximo ms; não usar o RTT local do Fedora
como RTT entre PCs. Gaps entre snapshots Windows 29.933 ms/Fedora 15.817 ms
não medem silêncio de todos os datagramas nem perda; causa não isolada numa
rodada com mudanças de foco e saída. Draw expira a pose em 500 ms, visual em
1 s: próximo alvo é aquisição/foco e timeline, sem fingir movimento por pose
antiga. Recarga LAN, reconexão, mais jogadores e sessão longa não passaram.

Stops dos dois PCs: 14 slots e oito hashes em cada. Fedora deixou worker
parado, item zero/gate null, dispatcher/pump removidos, filas zeradas e leave
confirmado; oito hashes passaram novamente após saída, enumeração sem jogo.
Servidor parado normalmente, zero ativos (cached_members=3 é cache), três
leaves incluindo uma sonda inicial sem pose. Windows sem segundo verify após
saída. Nenhuma DLL residente no jogo local; reenumerar antes do próximo build.
Não houve mudança de código/rebuild neste registro, só coleta/documentação.

Depois dessa rodada o proprietário pediu explicitamente reutilizar os logs e
implementar offline, reduzindo o uso do desktop e do laptop da esposa. Não
abrir jogos nem pedir nova conexão para cada incremento. `assistant_paths`
consulta Documents via Known Folder Windows (memória CoTaskMemFree), expande
variáveis sem shell/recursão e valida caminhos com erros específicos. `mp_lan`
lembra preferências privadas, simplifica mensagens, salva status automaticamente
no início/antes/depois do stop e exporta apenas campos tipados em diagnosticos.
`session_report` analisa status/stop antigos/novos por um peer em ordem explícita,
sem processo/injeção; logs reais da primeira LAN foram reprocessados. Não
publicar raw logs/identidade/backups no Git nem chamar replay offline de novo
teste LAN. Controle ABI v1 e engine ABI v3/664 preservados; captura/render/gate
e o sumiço por foco não foram alterados. Consultar a validação deste bloco no
roadmap antes de instalar outro kit; a revisão do assistente no Windows real
fica para marco agrupado, não requer nova prova só para o analisador.
