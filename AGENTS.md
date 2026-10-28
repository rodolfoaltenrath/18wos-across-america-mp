# Orientações para continuar o projeto

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
