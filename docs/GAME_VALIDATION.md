# Rodada acumulada de validação na partida

Atualizado em 29/10/2026. O proprietário pediu para desenvolver sem abrir o jogo
agora e concentrar a validação prática depois. Este é o roteiro acumulado do
que já está implementado ou depende de preparação específica; não é registro
de aprovação. Acrescentar novas entregas aqui antes da próxima rodada.

As provas antigas permanecem no ROADMAP.MD: eco/textura e duas cargas `map`
passaram no Windows/v2; o usuário percebeu atraso leve. Houve reconexão real,
mas ele não percebeu o desaparecimento do remoto durante a interrupção. Outros
comandos de teardown, suavização na LAN, vários jogadores e sessão longa
continuam pendentes. Testes em fixture/sockets locais não fecham essas lacunas.

## Preparação comum

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
| G5 | LAN com dois PCs, vários remotos e plataformas | Pendente; exige outras máquinas funcionais |
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

- [ ] No destinatário Windows, copiar o kit local inteiro para a raiz e testar
  `mp_lan.exe`: checagem do perfil/hashes, PID correto, confirmação da pasta
  real de saves, backup, identidade nova, início/status/parada e logs.
  Recuperar a parada ao reabrir o assistente e verificar após a saída.
  A bancada não comprova o fluxo de injeção do novo assistente nesse PC.
  Servidor no Fedora, sem eco; mesma sala e offset zero nos dois clientes.
- [ ] Dois PCs com jogo original funcional, mesmo perfil/mapa e builds compatíveis;
  sala v2 em bind LAN, endpoint configurável e identidades locais diferentes.
  Não há lista de servidores nem negociação automática de mapa/mods entregue.
- [ ] Sem `--echo`, cada jogador vê o OUTRO dirigir independentemente. Medir
  ambos os sentidos, curvas/paradas, entrada/saída e reconexão de um só PC.
- [ ] Conferir que o remoto não altera o caminhão local ou save. Mais de dois
  jogadores exige PCs/clientes reais adicionais; eco não substitui essa prova.
- [ ] Windows↔Linux/Proton quando o jogo Linux estiver funcional. Separar falha
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

- [ ] Primeiro implementar/revisar montagem e prioridade do overlay com backup e
  restauração de `aa/init.cfg`. `mod-stage` NÃO ativa um pacote na Steam.
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

Só atualizar checkboxes após evidência correspondente. PT-BR/R$, economia,
contratos, carreira configurável e demais features ainda não implementadas não
entram como testes reprovados: permanecem tarefas de desenvolvimento no roadmap.
