# Rodada acumulada de validação na partida

Atualizado em 30/10/2026. O proprietário pediu blocos maiores e validação prática
agrupada. A primeira rodada LAN entre Windows e Fedora/Proton foi realizada e
está registrada abaixo. A matriz restante continua sendo roteiro de pendências;
não marcar itens amplos a partir de uma prova parcial.

As provas antigas permanecem no ROADMAP.MD: eco/textura e duas cargas `map`
passaram no Windows/v2; o usuário percebeu atraso leve. Houve reconexão real,
mas ele não percebeu o desaparecimento do remoto durante a interrupção. Outros
comandos de teardown, suavização na LAN, vários jogadores e sessão longa
continuam pendentes. Testes em fixture/sockets locais não fecham essas lacunas.

## Incremento offline de carreira — 29/10/2026

Sem abrir os jogos: `mp_career` passou em transferência TCP de save-base e
consulta do estado próprio, manifesto/regras/mapa/mods, ledger de contratos,
checkpoints e recuperação. Verify: 183 testes x86 sob Proton e 214 nativos,
releases, Clippy e fmt. Lote de 1.000 ciclos por alvo nativo/Windows x86: cada
um com 7.000 operações persistidas, 3.000 reaberturas de recuperação e 1.000
transferências de fixture; relatórios iguais. ACK perdido e reinício do store
são simulados; sockets/arquivos/locks/hard links são reais. O controle TCP desse
bloco ainda não era ligado à admissão UDP/launcher; a extensão de 30/10 está em
G9. Nenhum deles está ligado ao mundo do motor. Não são testes novos
de saves comerciais ou LAN entre PCs. Atestado de entrega vem do host/fixture;
não é leitura de chegada no jogo. Mais detalhes no novo registro do roadmap.

Os sete relatórios Fedora e dois Windows preservados abaixo foram novamente
analisados; saídas byte a byte iguais às anteriores. Captura por draw, sumiço
sem foco, render/gate/ABIs e limitações da primeira LAN permanecem os mesmos.
Nenhum kit instalado foi substituído. Não repetir a prova visual básica para
validar esta ferramenta: o próximo teste humano exige adaptadores concretos.

## Registro — primeira LAN Windows ↔ Fedora/Proton — 29/10/2026

**Resultado confirmado:** o proprietário viu o caminhão do outro jogador nos
dois computadores. Ao tirar o foco, o remoto desaparece; devolvendo o foco,
reaparece. Isso é prova visual entre máquinas distintas, sem eco nem cliente
sintético enviando poses. O Fedora capturou poses/rotações variando do Windows;
o log Windows contém a pose/rotação recebida do Fedora. Não repetir essa prova
básica apenas para preencher documentação. Novos testes devem cobrir regressão
de mudanças concretas ou lacunas especificadas ao final deste registro.

### Ambiente e procedência

- Build de origem/kit: `7f2595c40ee6cde875d0a9031348b0137423d069`.
  DLL/launcher/assistente x86 compilados no Fedora; servidor Rust nativo Linux.
  Os logs Windows não incluem nova medição do hash da DLL do mod no destino.
- Perfil nos dois PCs: `steam-aa-1.10-ccb327c2`; os cinco originais do backup
  Windows correspondem aos hashes perfilados. Doctor Fedora passou oito arquivos
  e a estrutura do prefixo antes da injeção. Assets/mapa inteiros não foram
  comparados; checks estáticos não substituem a prova executada nesta rodada.
- Fedora: kernel `7.2.13-200.fc44.x86_64`, Proton Experimental `11.0-100`.
  Configuração lida: `r_device=dx8_0`, `r_fullscreen=0`,
  `r_mode=1024x768x32x85`. É configuração, não medição de FPS/refresh efetivo.
  Módulos observados incluem `d3d8.dll` e `wined3d.dll`; não inferir custo de GPU.
- Windows: versão exata do SO, hardware, renderizador e resolução não coletados.
  O save estava em Documents redirecionado ao OneDrive/Documentos. O assistente
  inicialmente recusou o caminho e o proprietário resolveu sem alteração de
  código; o texto exato da entrada recusada não foi preservado. Melhorar a
  descoberta de Documents e o diagnóstico do caminho é uma pendência de UX.
- Sala `lan`, wire v2, servidor no endereço da interface LAN do Fedora, UDP
  39000, limite 16, timeout 5 s, `echo=false`. Firewall já permitia a porta;
  nenhuma regra foi alterada. Offset visual zero nos dois PCs; apresentação
  100 ms de interpolação / 200 ms de extrapolação.
- IDs desta sessão: Windows 2, Fedora 3. O ID 1 era uma sonda nativa de dois
  segundos com `--no-pose`, encerrada antes da sessão visual; não é terceiro
  jogador. Identidades independentes. Não versionar identidade, paths locais,
  saves ou os arquivos comerciais recebidos junto dos logs.

### Rede e apresentação — valores brutos preservados

Os snapshots não foram coletados no mesmo instante nem sincronizados entre
relógios. Coluna Fedora usa o estado após a parada, com contadores mantidos;
Windows usa o status anterior à parada. A saída foi capturada separadamente.
São contadores/estimativas locais, não um benchmark de percurso controlado.

| Campo | Fedora | Windows |
| --- | ---: | ---: |
| Enviados / recebidos (`client`) | 22.197 / 24.028 | 27.089 / 19.561 |
| Aceites / reconexões | 1 / 0 | 1 / 0 |
| Inválidos / recusas da sessão | 0 / nenhuma | 0 / nenhuma |
| `old` (descarte do protocolo, não perda estimada) | 449 | 972 |
| RTT atual / mínimo / máximo (ms) | 0 / 0 / 17 | 17 / 16 / 34 |
| Amostras RTT | 984 | 878 |
| Variação RTT suavizada (`rtt_jitter_us`) | 1.880 | 2.183 |
| Snapshots admitidos pelo recorder | 24.477 | 20.539 |
| Variação de chegada suavizada (µs) | 1.120 | 22.787 |
| Maior intervalo entre snapshots (ms) | 15.817 | 29.933 |
| Eventos de salto de tick / maior passo | 1 / 949 | 4 / 1.796 |
| Snapshots não monotônicos | 0 | 0 |
| Consultas interpoladas / previstas | 33.493 / 15 | 21.102 / 51 |
| `buffering` / `capped` | 7 / 54 | 24 / 177 |
| `stale` / `empty_queries` | 311 / 973 | 929 / 6.229 |
| `lock_misses` / `held` / `snapped` | 6 / 0 / 0 | 4 / 0 / 0 |

O RTT Fedora é até o servidor no próprio computador, não o RTT Windows↔Fedora.
`clock_offset_ms` era diagnóstico relativo à época do servidor, não latência:
677.759 no Fedora final e 644.904 no Windows. Não usar a diferença para estimar
atraso de ponta a ponta ou afirmar relógios sincronizados.

`max_receive_gap_ms` mede intervalos **entre snapshots**, não silêncio de todos
os datagramas. Saltos de tick não medem perda. As lacunas ocorreram numa rodada
com alternância de foco/consoles e remoção do peer, sem trace que isole a causa;
não atribuir automaticamente a defeito do UDP, perda física ou freeze integral
do processo. Antes das lacunas, os snapshots Fedora preservados registraram
maior intervalo 33–34 ms e maior passo 1. Preservar também os valores finais
maiores; não selecionar apenas os bons trechos.

Poses individuais e quaternions completos estão nos JSONs. Exemplos Fedora:
remoto Windows começou em `[-1127781.875, 19977.296875, 273040.375]`, sequência
2.465; depois `[-1126020.5, 19977.50390625, 273258.6875]`, sequência 8.780;
na segunda coleta de métricas, `[-1128268, 19990.048828125, 272233.09375]`,
sequência 23.794. São centímetros do mundo, não coordenadas geográficas.
O Windows registrou o Fedora em `[-1127316.875, 19977.681640625, 273321.6875]`,
sequência 20.298. Ambos tinham rotação e buffer de 16 amostras; idade do remoto
Fedora 0–13 ms nas coletas com peer, Windows 72 ms naquele snapshot.

### Desenho, loop e memória

| Campo | Fedora | Windows |
| --- | ---: | ---: |
| Thread proprietária observada (histórica) | 400 | 21.304 |
| Desenhos remotos | 33.569 | 21.354 no status; 21.455 antes de liberar |
| Custo médio de draw no último diagnóstico (µs) | 1,950 | 5,026 |
| Maior custo de draw registrado (µs) | 161,100 | 266,300 |
| Intervalo médio observado no hook (µs) | 4.234 | 6.427 |
| Partes com shader | 3 | 3 |
| `fault` / acesso residual / chamadas estrangeiras / flags | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| Desenhos locais / flushes no snapshot correspondente | 103.710 / 414.876 (parado) | 82.761 / 331.044 |
| Draws pulados (`skipped`) | 70.075 | 61.407 |

No Fedora, os custos médios expostos oscilaram entre 1,487 e 2,259 µs nas
coletas anteriores; não são média ponderada da sessão. O intervalo do hook
também não é FPS medido do jogo inteiro. `skipped` inclui passes/contextos
inelegíveis, não quadros necessariamente perdidos. Três partes com shader não
substituem inspeção específica de cores/iluminação em todas as câmeras; o relato
do proprietário confirma presença mútua e foco/retorno, não toda a matriz G1.

Sonda de render Fedora, snapshot próprio: 703.124 chamadas pool / 290.414
flushes, flags e foreign zerados. É cobertura do ponto perfilado, não todos os
passes. Loop antes do stop: thread da janela/execução/fase 400, nenhuma tarefa
pendente/em curso. Preflight visível recusou por `visible`, em `[1,1,0]`,
profundidades pump/janela/draw/render `1/0/0/0`, sem ocultar/liberar.

O contador rotulado `read_failures` no Fedora atingiu 3.222 numa coleta; ele
também soma `SampleError::Unavailable` da fonte `drawn`, cujo diagnóstico foi
`waiting for a drawn exterior pose (game in foreground)`. Não classificá-lo
inteiro como falha de leitura de memória. O watch preservou transição de
`sample=2` para `sample=1`, além da recuperação visual relatada.

Observações do SO local (coleta curta, sem baseline antes da injeção): jogo
RSS 453.800–453.804 KiB (~443,2 MiB); servidor RSS 4.012 KiB (~3,92 MiB).
Duas amostras intervalares do top: jogo 229,8% e 226,3% CPU, servidor 0,5%
em cada uma; percentual é por núcleo, inclui jogo/Wine, não apenas o mod.
Não há medição de CPU/RSS Windows, GPU/VRAM, crescimento de heap ou FPS isolado.
VirtualQuery Fedora: regiões graváveis committed PRIVATE 328.327.168 bytes
(313,12 MiB) e IMAGE 3.309.568 bytes (3,16 MiB). Não é RSS nem todo o heap.
Lista de módulos, maiores regiões e estado do processo foram preservados.

### Servidor e encerramento

Relatório final do servidor: recebidos 49.454; poses 47.548; pings 1.900;
ticks 72.598; snapshots 45.870; enviados 48.727 / 15.466.828 bytes. Inválidos,
oversized, rate_limited, recusados, stale, unauthenticated, expirados,
send_errors e late_ticks: todos zero no relatório. Aceites/leaves: 3/3,
incluindo a sonda sem pose. `active_on_shutdown=0`, `cached_members=3`;
cache de retomada não é cliente ativo nem prova de toda memória liberada.

- Windows: log de parada prova 14 slots restaurados, oito arquivos iguais ao
  backup (cinco originais/três saves), worker status 2, uma consulta de preflight
  e uma tentativa de release, sem recusas; `[1,1,0]`, 12 flushes após ocultar e
  21 após último draw. Retorno na thread 21.304, profundidades `1/0/0/0`.
- Fedora: 14 slots restaurados, oito arquivos iguais (cinco originais/três
  saves), uma tentativa de release após 118 consultas de preflight; aguardou
  consumo de draws com recusa retryable, sem forçar liberação. Último check
  pronto em `[1,1,0]`, 108 flushes após ocultar / 1.089 após último draw.
- Estado posterior Fedora: worker 2/parado, `leave_confirmed=true`, sessão
  `closed`, sem remotos, visual `item=0/shown=0/state=0`, gate null;
  dispatcher inativo, loop removido (`pump_state=2`), tarefas pending/running
  e inflight zero. O `load_gate.installed=true` no relatório **de parada** é
  snapshot anterior à remoção; o status posterior confirma gate ausente.
- Enumeração após saída do Fedora não encontrou o jogo e a verificação
  independente de PID/DLL confirmou novamente os oito arquivos iguais.
  Servidor encerrado normalmente, socket removido. Não há DLL residente no
  jogo local ao concluir; reenumerar antes de futuros builds.
- No Windows, o proprietário relatou que o assistente liberou o fechamento;
  os logs comprovam a limpeza antes da saída. Não foi coletada uma segunda
  verificação pós-saída nem enumeração remota do processo. Isso não exige
  repetir o percurso LAN: a verificação pode ser feita independentemente do jogo.

### Evidências e próximos testes específicos

Originais de diagnóstico locais em `target/analysis/lan-two-pc-20261029-01/`
(ignorado): status inicial/pós-movimento/duas métricas/saída do par/parado,
watch de 60 s, render/pump/dispatcher/controle, preflight, módulos/regiões/SO,
configuração de vídeo selecionada, hashes dos builds, backup/stop/verificação
pós-saída e relatório final do servidor. Os três logs Windows de início,
status e parada foram copiados para `windows-start/status/stop.json`.
Os arquivos completos fornecidos pelo proprietário permanecem privados fora
do Git. O pacote de evidências em Downloads inclui somente a lista explícita
de relatórios/logs; exclui identidades, backups, saves e arquivos do jogo.

Pendências delimitadas, sem repetir a prova já obtida:

1. Separar atualização/envio de pose da disponibilidade de draw/foco, com prova
   de thread e lifecycle; não manter pose antiga como se fosse movimento atual.
   O contrato atual expira a fonte após 500 ms e não desenha remoto após 1 s.
   Regressão futura deve mirar perda/retorno de foco e transições de mundo.
2. Isolar as lacunas de snapshots com timeline monotônica de foco, fonte,
   envio/recepção/tick, sem concluir perda de pacotes pelos contadores atuais.
3. Quantificar suavidade, latência visual ponta a ponta e FPS/custo incremental
   em trecho controlado; não foram fornecidas notas específicas de conforto,
   saltos, câmeras, curvas/rampas, textura/iluminação nesta rodada.
4. Reconexão/entrada/saída percebida de um peer, vários jogadores (>2), menus/
   recargas na LAN e sessão longa permanecem específicos. Gate ficou idle,
   sem calls/replays/recriações; não houve novo teste de reload neste bloco.
5. Descobrir/persistir Documents real, explicar caminho inexistente/arquivo/
   permissão no assistente e melhorar a clareza de Status/Parar/retorno ao jogo.
   O fluxo interativo Windows passou nesta rodada; console via Proton continua
   fora deste teste. Não solicitar novas voltas/menu/load só por essa melhoria.

## Preparação comum

Revisão offline posterior à primeira LAN (sem novos jogos): `mp_lan` agora
consulta Documents real, lembra a pasta/endpoint/sala, explica erros de caminho
e salva status automaticamente no início/antes/depois do stop. Gera subpasta
`diagnosticos` tipada e separada dos dados privados; `session_report` analisa os
relatórios antigos/novos offline, com limites e sem inferir foco/perda/limpeza
real a partir dos JSONs. Os logs desta rodada já foram reutilizados como
entradas, além de fixtures próprias de indisponibilidade/recuperação, mudança
de epoch, regressão/wrap e slot contraditório. O fluxo novo no Windows real
e as coletas automáticas ainda aguardam o próximo marco agrupado. Não repetir
o percurso LAN básico apenas para testar o analisador ou a descoberta de pasta.

1. Registrar commit, builds, sistema, renderizador, resolução, foco da janela,
   versão do jogo/perfil por hash e configuração do servidor. Compilar/conferir
   antes de abrir o jogo; nunca reconstruir a DLL enquanto estiver residente.
   Executar primeiro `mp_doctor` nativo com `--game` e `--client-dir` corretos
   (servidor/prefixo opcionais conforme o README) e guardar o relatório redigido.
   Aprovação estática não comprova runtimes, ABI ao vivo ou igualdade de builds.
2. Usar save de teste existente, sem substituir/salvar por cima. Separar as
   identidades e os artefatos em `target/analysis/rodada-NOVA/`, ignorado.
3. Conferir PID atual e paths. No Windows, launcher/DLL x86 em
   `target/i686-pc-windows-gnu/release/` e servidor/assets nativos em
   `target/x86_64-pc-windows-gnu/release/`. No Linux, usar launcher no mesmo
   prefixo/runtime da partida; não executar helper Windows como binário Linux.
4. Preparar backups novos por `session-start --backup`; isso copia saves e
   hasheia os cinco originais perfilados, não toda a instalação. Guardar status
   inicial/final e hashes. A identidade é privada e diferente em cada PC.
5. Manter primeiro plano nos trechos medidos: sem foco, render e envio pela
   fonte `drawn` podem congelar. Não atribuir esse caso ao protocolo/gate.

Referência de sessão com eco v2 em um PC (placeholders PID/DLL/BACKUP/IDENTIDADE):

```text
multiplayer_server --bind 127.0.0.1:39000 --room validacao --echo
mp_launcher --pid PID --dll DLL session-start --server 127.0.0.1:39000 --room validacao --identity-file IDENTIDADE --offset 400 0 0 --backup BACKUP
mp_launcher --pid PID --dll DLL session-status
```

Capturar a saída em arquivos locais novos. Não compartilhar identidades,
credenciais, saves, dumps ou assets. Os comandos de preparação/limpeza completos
e o fallback estão no README; não usar PIDs ou endereços históricos.

## Matriz e ordem da rodada

| ID | Cobertura | Situação antes da rodada |
| --- | --- | --- |
| G1 | Visual/pose/skin e funcionamento básico | Regressão; passou em sessão anterior |
| G2 | Ajustes de interpolação/extrapolação e métricas novas | Bancada; nova prova na partida pendente |
| G3 | Interrupção/reconexão e desaparecimento percebido | Recuperação anterior passou; visibilidade pendente |
| G4 | Menus e recargas pelo gate/fallback | `map` anterior passou; ampliar matriz |
| G4b | `exit`/`menu`/`quit`/`preview`/`edit` | Bancada; exige preparar cada caminho real |
| G5 | LAN com dois PCs, vários remotos e plataformas | Visual mútuo Windows↔Fedora passou; ampliar conforme registro acima |
| G6 | Sessão longa, custo e retenção de recursos | Pendente |
| G7 | Pacotes/receitas/cores no motor e restauração da instalação | Pipeline offline pronto; ativador ainda não implementado |
| G8 | Encerramento, restauração de hooks e integridade | Obrigatório em cada sub-sessão, inclusive falhas |

Não iniciar os trechos dependentes de outro ainda reprovado. G4b e G7 exigem
preparo técnico antes da ação humana; não são instruções para experimentar
comandos desconhecidos ou editar originais agora. Pode ser necessário usar
processos novos entre grupos; rodada agrupada não significa uma única injeção.

### G1 — Visual básico

- [ ] Textura/cores/iluminação corretas no primeiro desenho e sem travamento.
- [ ] Frente/ré, curva para ambos os lados, parada, rampa e câmeras disponíveis;
  observar posição, orientação, teleporte inesperado, sumiço e artefatos.
- [ ] Conferir que o visual não controla/empurra o jogador nem cria HUD/física.
  Colisão, rodas/reboque/sinais próprios e modelo por jogador não foram entregues.
- [ ] Capturar `session-status` antes/depois: remotos, visual, contadores e custo.

### G2 — Suavização e métricas

- [ ] Medir primeiro os padrões 100 ms de atraso / 200 ms de extrapolação,
  repetindo percurso curto com paradas e curvas. Registrar percepção e FPS.
- [ ] Repetir com 50/100 e 0/50 usando `presentation --delay-ms N
  --extrapolation-ms N`, sem concluir que menor atraso sempre é melhor.
- [ ] Conferir configuração/revisão e modos em `presentation`; `network-timing`
  contém métricas v2 e retorna null em v1. Restaurar 100/200 no fim da comparação.
- [ ] Comparar `draw_cost_*`/intervalo de frame e timing com mesma câmera/foco.
  RTT/jitter/gaps não são medição de FPS nem estimativa automática de perda.
- [ ] Na LAN, repetir com pausa/atraso de envio e observar previsão curta e
  retenção após o limite, sem movimento indefinido. Proxy é opcional e local.

### G3 — Rede e reconexão

- [ ] Interromper o servidor por tempo maior que o timeout e observar de fato
  se o remoto deixa de ser desenhado. Status remoto zero sozinho não comprova
  o que apareceu na tela. Registrar separadamente observação e contadores.
- [ ] Reiniciar a sala com o mesmo endpoint; confirmar conexão nova, lista
  coerente e visual com textura/movimento, sem instância antiga/duplicada.
- [ ] Desconectar/reconectar um cliente; conferir que o outro permanece correto.
  Sem salvar, encerrar normalmente e verificar confirmação de saída.
- [ ] Repetir v1 de compatibilidade em sub-sessão separada; não misturar
  resultados ou credenciais dos dois protocolos.

### G4 — Menus e carregamento

- [ ] Com gate padrão e visual ativo: menu de carregar por 10 s, save de teste,
  retorno à partida, dirigir/parar; repetir e alternar volta ao menu principal.
- [ ] Após cada carga, capturar gate/visual: uma reexecução por comando aceito,
  argumentos restaurados, liberação anterior e recriação; textura correta.
- [ ] Conferir recusas/timeouts/faults, tarefas pendentes e recuperação. Só
  declarar um comando exercitado se o descritor/contador correspondente mudou.
- [ ] Sub-sessão separada com `--no-load-gate`: `session-prepare-load`, confirmar
  `prepared=true` e item zero ANTES do menu/load; recriar explicitamente conforme
  README. Nunca repetir a recarga com instância retida e sem proteção.
- [ ] Não instalar sondas passivas `transition-*` junto do visual retido;
  capturas passivas adicionais exigem sessão própria sem visual.

### G4b — Outros caminhos de teardown

- [ ] Mapear cada ação real de UI ao descritor `exit`/`menu`/`quit` e comprovar
  cobertura; navegar por um menu não prova que o comando `menu` foi chamado.
- [ ] Preparar `preview`/`edit` em ambiente isolado e revisar possíveis escritas
  do editor antes de exercitá-los; não adivinhar argumentos/comandos no console.
- [ ] Antes de testar saída com gate ativo, preparar observação/limpeza adequada
  à morte do processo e rollback. A orientação atual permanece `session-stop`
  ANTES de fechar o jogo; não testar esse risco apenas para marcar a checklist.
- [ ] Novo jogo, morte/reinício e fechar a janela não são interceptados. Criar
  tarefas de integração específicas antes de incluí-los como cargas protegidas.

### G5 — LAN e plataformas

- [x] No destinatário Windows, copiar o kit local inteiro para a raiz e testar
  `mp_lan.exe`: checagem do perfil/hashes, PID correto, confirmação da pasta
  real de saves, backup, identidade nova, início/status/parada e logs passaram.
  Recuperação ao reabrir e verificação Windows após saída seguem pendentes.
  Servidor no Fedora, sem eco; mesma sala e offset zero nos dois clientes.
- [ ] Próxima revisão do assistente: confirmar Documents redirecionado sem
  digitar caminho, preferência persistida, mensagens simples e status/exportação
  automáticos. Isso pode ser agrupado com a próxima prova de correção do motor;
  consultas estáticas e análise dos logs não exigem nova partida LAN.
- [x] Dois PCs com jogo original funcional, perfil suportado e kit compatível;
  sala v2 em bind LAN, endpoint configurável e identidades locais diferentes.
  Não há lista de servidores nem negociação automática de mapa/mods entregue.
- [ ] Sem `--echo`, cada jogador vê o OUTRO dirigir independentemente. Medir
  ambos os sentidos, curvas/paradas, entrada/saída e reconexão de um só PC.
  Presença visual mútua e poses/rotações dos dois lados passaram; permanece
  aberta a matriz inteira de movimento controlado e reconexão.
- [ ] Conferir que o remoto não altera o caminhão local ou save. Mais de dois
  jogadores exige PCs/clientes reais adicionais; eco não substitui essa prova.
- [x] Primeira sessão visual Windows↔Linux/Proton. Separar falha
  de instalação/Proton de falha de rede; build cruzado não valida a partida.
  O doctor estático já passou também no Fedora em 29/10; isso não valida
  execução na partida, loader nem tráfego entre computadores.

### G6 — Duração e custo

- [ ] Sessão de 30–60 min com movimento, períodos parado e recargas previstas;
  observar controles/câmera/UI, consumo de memória/CPU/rede e custo de desenho.
  `session_soak` agora mede sala/clientes reais em loopback, sem jogo, no mesmo
  processo. Seus resultados de 16/32 clientes e recuperação não fecham G5/G6,
  não medem heap/FPS/render e não comprovam desaparecimento visual.
  O reteste Fedora passou 16 clientes/60 s e 32/30 s; churn agora aguarda a
  malha observada e registra ticks adiados. As mesmas limitações permanecem.
- [ ] Comparar pontos equivalentes após várias recargas; distinguir aquecimento
  de cache de crescimento repetido. Medir no mesmo mapa/câmera/foco.
- [ ] Rede indisponível prolongadamente e recuperação. Encerramento abrupto só
  em sub-sessão preparada com backup/diagnóstico; não provocar no save normal.

### G7 — Assets no motor (aguarda ativador)

- [ ] Revisar no motor prioridade/montagem do overlay. `ui_tools` já fornece
  ativação restrita ao menu próprio, backup e restauração de `aa/init.cfg`;
  isso não é um ativador genérico. `mod-stage` NÃO ativa um pacote na Steam.
- [ ] Escolher alteração mínima com formato/nome/material que o motor use;
  o PNG/texto do exemplo é fixture e não promete mudança visível na partida.
- [ ] Conferir cores, transparência, orientação, resolução e material antes/
  depois de reconstruir por receita. DDS/mipmaps e formatos proprietários não
  suportados dependem de trabalho próprio, sem conversão silenciosa.
- [ ] Desativar/remover e conferir restauração dos arquivos envolvidos, inclusive
  configuração de montagem. Pacotes derivados são locais e não vão ao GitHub.

### G8 — Limpeza e evidência

```text
mp_launcher --pid PID --dll DLL session-stop --verify BACKUP
# Fechar o jogo normalmente, depois conferir sem abrir o processo:
mp_launcher --pid 0 --dll DLL session-verify --backup BACKUP
```

- [ ] Conferir worker parado, item ausente, filas vazias, hooks removidos e os
  14 slots esperados restaurados. Falha de remoção impede nova injeção/build
  naquele processo; a DLL do pump só é liberada quando o processo termina.
- [ ] Confirmar saída normal/ausência do processo, verificar originais/saves e
  encerrar os auxiliares. Preservar erro/status/artefatos privados em caso de falha.
- [ ] Registrar por ID: commit/ambiente, parâmetros, resultado (`passou`,
  `falhou`, `não exercitado` ou `bloqueado`), observação humana e arquivo de
  evidência. Contadores, screenshots e hashes não são provas intercambiáveis.

### G9 — Entrada de carreira (nova implementação offline, 30/10)

Preparação já disponível sem abrir jogo: `mp_career host`, `join`,
`prepare-launch`, `check-launch` e preflight do launcher. TCP/UDP de produção
com fixtures próprias não comprovam o cenário carregado nem economia compartilhada.
Verify completo Windows em 30/10: 192 testes x86 e 227 nativos, builds release,
Clippy e fmt passaram. Inclui CLI real do launcher recusando divergências antes
da DLL/processo/backup e dois clientes TCP+UDP; nenhuma partida foi aberta.
Não reunir os dois PCs apenas para repetir a presença visual já comprovada.
Executar quando seleção pelo assistente e leitura do motor formarem um marco
concreto; até lá, estas pendências permanecem abertas.

- [ ] Repetir nativo Linux/Windows e x86/Proton o fluxo de preparo com dados
  próprios; testar mensagem de ticket recusado no launcher/DLL desta revisão.
- [ ] Confirmar carreira/manifesto/identidade/sala/endpoint, mapa/mods iguais e
  backup antes da injeção. Divergência deve recusar antes de abrir o processo.
  Não modificar originais para testar rejeição; usar cópias próprias.
- [ ] Demonstrar qual save/cenário o motor realmente carrega; verificação de
  arquivo em staging não é essa prova. O fluxo atual não instala o save.
- [ ] Em sessão agrupada, confirmar poses/visual e limpeza com ticket, sem
  interferência na carreira/saves locais. Relógio/saldo continuam locais.
- [ ] Exercitar recusa após expiração/reinício do host e o procedimento de
  parar, preparar ticket novo e iniciar cliente novamente. Sem refresh automático.
- [ ] Manter convites, tickets, identidades, recibos e saves fora dos exports
  de diagnóstico/Git; compartilhar somente evidência tipada sem credenciais.

Só atualizar checkboxes após evidência correspondente. PT-BR/R$ e integração
de economia/contratos/carreira configurável no motor permanecem tarefas de
desenvolvimento. O sidecar offline não fecha esses testes de gameplay.

### G10 — Telas e fontes locais (base entregue, 30/10)

Não precisa do segundo PC. `mp_ui` é janela própria de prévia, sem jogo/rede;
`ui_tools` gera ESC experimental e monta somente seu pacote exato com backup.
Os nove layouts ainda não têm adaptador nativo e o Multiplayer do menu do jogo
fica inativo. Nenhum teste de GDI/fixture fecha os itens abaixo. Procedimento,
comandos e limitações em README e docs/UI_DESIGN.md.

Verify Windows passou: 196 x86 e 243 nativos, releases, Clippy e fmt. Instalar/
restaurar os arquivos reais do pacote inicial 0.1.0 com jogo fechado também
passou: config original
com hash igual ao backup, overlay ausente no fim. Nove BMPs próprios foram
gerados e o teste de glifos/GDI passou. Não houve abertura do jogo, interação
humana com a prévia, nova LAN ou execução Linux/Proton neste incremento.
O ajuste posterior ao painel esquerdo 0.2.0 repetiu verify com os mesmos totais;
BMP principal inspecionado, pacote gerado, sem instalar ou abrir o jogo.

- [ ] Abrir a prévia, navegar por mouse/Tab/Escape, substituir inputs com Ctrl+A;
  conferir metas, sem limite, mensagens e troca PT-BR/inglês. Redimensionar e
  conferir clique/foco/legibilidade, inclusive escala de tela do Windows.
- [ ] Com jogo fechado e backup novo, instalar o menu experimental. Abrir pela
  Steam e confirmar que a montagem p4 é aceita, o fundo/textos aparecem e não
  há erro de recurso. Não iniciar uma sessão multiplayer por esse protótipo.
  Usar pacote 0.2.0 ou posterior: somente o retângulo esquerdo deve mudar;
  cenário animado, logo superior e logotipos inferiores devem continuar visíveis.
  Conferir semitransparência, posição dos seis itens e hover das ações antigas.
- [ ] Conferir cinco ações antigas (nova carreira sem criar save, carregar sem
  sobrescrever, opções, créditos e sair), retorno e resoluções/fullscreen.
  Não assumir suporte a outros cenários além do que o handler original oferece;
  restringir cenários à carreira financeira ainda depende de implementação.
- [ ] Fechar normalmente, restaurar pelo backup e confirmar config original
  byte a byte e overlay ausente. Reabrir e conferir menu original.
- [ ] Depois de implementar o adaptador: botão Multiplayer funcional, edição
  nativa, trabalho sem bloquear UI, cancelamento e retorno, admissão/carreira/
  save/limpeza reais. Ainda tarefa de desenvolvimento, não só teste pendente.
- [ ] Depois das telas: tradução integral, glyphs/atlas/codepage nativos,
  placeholders/PML e layout em menus/HUD/diálogos. Prévia Unicode não valida
  fontes do jogo; ASCII provisório não é tradução final.

Registrar observação humana, build/perfil/resolução/escala e resultado por item.
Nova prova LAN só para o fluxo integrado quando houver dois computadores.

### G11 — Tradução PT-BR da tabela e fontes (T1 entregue offline, 30/10)

Não precisa do segundo PC. Pacote `ptbr-text` gerado localmente: 484/484 chaves
de `scripts/local.def` e 21 glifos sintetizados em 13 fontes. Comandos e limites
em docs/LOCALIZATION.md. Prévia PNG inspecionada fora do jogo; nenhuma
montagem/abertura feita. Dublagem é etapa própria (T3), fora desta rodada.

- [ ] Com jogo fechado, `ui_tools install --kind ptbr-text` com backup novo.
  Abrir pela Steam: confirmar que a montagem p5 é aceita e que menu, opções e
  carregamento aparecem em PT-BR.
- [ ] Acentos: conferir à, â, ã, ç, ê, õ, maiúsculas acentuadas e ú nas fontes
  pequena (dicas/HUD), média (menus) e grande (títulos/placas). Registrar se
  algum glifo some, sai deslocado ou aparece como outro caractere: isso
  refutaria a hipótese de um byte por caractere (Windows-1252).
- [ ] Layout: textos mais longos que o inglês em botões, opções de controle,
  Caderno (economia/frota), loja e mensagens de entrega. Anotar cortes.
- [ ] Chaves com cor (`qh_city_buttonx`, `qh_cargo_listx`) e `<br>` em
  `ui_gfx_changed` (trocar resolução) exibem cor/quebra corretas.
- [ ] Textos fora da tabela continuam em inglês (esperado até T2); anotar onde
  aparecem para priorizar.
- [ ] Fechar normalmente, `ui_tools restore` e confirmar config original e
  overlay ausente. Com menu experimental também montado, restaurar na ordem
  inversa da instalação.
