# Protocolo de sessão LAN v2

O controle de carreira/save-base é um canal separado (`mp_career`, TCP 39001).
A admissão optativa acrescenta somente um ticket ao Join UDP v2, descrito abaixo.
No TCP, frames JSON têm prefixo de comprimento
u32 big-endian (1..262144), uma solicitação por conexão, prazo total de 30 s,
até oito conexões simultâneas. Convite privado contém schema/career/hash do
manifesto/segredo de 128 bits. Solicitação envia schema/career/segredo e
fingerprints de compatibilidade; resposta recusa ou fornece manifesto e
exatamente o blob de tamanho/hash declarados, seguido de EOF. Nenhum path
remoto é aceito. Cliente confere também o hash do manifesto fixado no convite.
Sem TLS/contas; restrito a LAN confiável. Fingerprints declarados não provam
assets carregados pelo jogo. A entrega do save não admite automaticamente o
jogador no relay nem torna relógio/física/trânsito compartilhados.

`action` opcional é `bootstrap` (padrão), `status` ou `admit` (host com relay).
`status` devolve manifesto
e snapshot validado do sidecar, sem blob, seguido de EOF. Snapshot informa
revisão persistente, clock/último checkpoint de clock, ranking e contratos;
omite histórico de operações e chaves. Clock é do sidecar, não o horário do
motor. Administração econômica ocorre somente em CLI local com lock exclusivo;
não há operação de crédito/entrega neste wire. Identidades da carreira/empresa
e credenciais do relay permanecem contratos distintos; `admit` vincula uma
declaração de identidade/importação ao ticket de entrada, sem registrar empresa
nem autorizar operações econômicas.

Contrato próprio em Rust. O codec v1 continua separado e disponível para as
provas anteriores. V2 é optativo: servidor com `--room NOME`; cliente com sala
e arquivo de identidade. Não há downgrade automático ou mistura de jogadores
v1/v2. Este contrato não comprova uma partida entre dois PCs.

## Sala e identidade

Cada processo do servidor cria uma sala com nome de até 32 caracteres ASCII
alfanuméricos, `-` ou `_`. Perfil de jogo é um identificador de compatibilidade
de até 64 bytes; por padrão `steam-aa-1.10-ccb327c2`. O servidor compara o rótulo,
sem precisar dos arquivos comerciais. Isso não verifica os bytes instalados no
cliente; os perfis/leituras do launcher e da integração continuam responsáveis
pela validação local dos módulos.

Cada cliente tem um identificador local de 128 bits, gerado pelo RNG do sistema
e salvo em JSON próprio. O arquivo é lido novamente nos próximos usos; arquivo
inválido não é substituído. Cada PC/jogador deve ter seu próprio arquivo, fora
do Git. Esse identificador não é senha, conta autenticada ou identificação do
save. Contas e autorização da futura economia são outra camada.

O ID de sala (`session`) muda ao reiniciar o servidor. O ID numérico de jogador
é atribuído pelo servidor; uma retomada preserva-o enquanto a identidade está no
cache da sala. O cache guarda no máximo 64 identidades; quando cheio, remove a
identidade inativa mais antiga. Não há promessa de preservar o ID numérico após
essa remoção ou reiniciar a sala. A identidade local continua a mesma.

Cada conexão tem número monotônico (`connection`) e token aleatório de 128 bits.
Credenciais são verificadas com o endpoint de origem. Uma retomada autenticada
pelas credenciais anteriores pode mudar o endpoint e sempre troca a conexão e
o token; datagramas anteriores deixam de ser aceitos. Outra origem sem essas
credenciais não derruba uma identidade ativa. O token só trafega nos pacotes,
não aparece nos diagnósticos/status e não é salvo no arquivo de identidade.
Não há criptografia do tráfego nem autenticação de contas nesta versão.

## Transporte e mensagens

UDP, envelope JSON `{ "version": 2, "message": { "type": "..." } }`, no
máximo 1200 bytes. Rejeitar estruturas/campos desconhecidos nos envelopes e
mensagens, tipos inválidos, NaN/infinito e quaternion não unitário (tolerância
herdada do v1). Nenhum pacote executa código, informa ponteiros ou chama o motor.

| Sentido | Tipo | Comportamento |
| --- | --- | --- |
| C→S | `join` | Nonce, sala, identidade, perfil e credenciais anteriores opcionais. |
| S→C | `accepted` | Correlaciona nonce; credenciais, jogador, revisão, timeout e relógio do servidor. |
| S→C | `rejected` | Correlaciona nonce; versão suportada e motivo tipado. |
| C→S | `pose` | Credenciais, sequência, relógio local do envio, posição e rotação opcional. |
| C→S | `ping` | Heartbeat com credenciais, sequência e instante local. |
| S→C | `pong` | Correlaciona conexão, sequência e instante; acrescenta relógio do servidor. |
| S→C | `roster` | Estado completo dos IDs remotos, com revisão da composição da sala. |
| S→C | `snapshot` | Até quatro poses com sala, revisão, tick e relógio do servidor. |
| C→S | `leave` | Saída da conexão identificada pelas credenciais. |
| S→C | `left` | Confirmação idempotente da saída. |
| S→C | `closing` | Aviso de encerramento, associado à conexão. Não substitui timeout. |
| C→S | `event` | Evento confiável: credenciais, `stream`, `id`, `ttl_ms` e corpo tipado. |
| S→C | `event_ack` | Admissão (primeira ou repetida) do `stream`/`id`; encerra o reenvio. |
| S→C | `relay` | Cópia sequenciada pela sala (`event`), origem e corpo; repetida até o recibo. |
| C→S | `relay_ack` | Recibo do `event` da sala; enviado para cada cópia recebida. |

Recusas: versão, sala, perfil, lotação, identidade ativa, endpoint ocupado e
encerramento. Versão/sala/perfil incompatíveis encerram a tentativa com erro;
lotação e identidade/endpoint ocupados usam nova tentativa com intervalo maior.
Não há autenticação econômica ou troca automática de servidor.

## Cadência, perda e reconexão

Tick de rede alvo: 60 Hz; não é tick comprovado da física. Sem coordenada válida,
o cliente deixa de enviar poses e mantém heartbeat a cada 500 ms. Timeout
configurável no servidor: 2–60 s, padrão 5 s. Pacotes com token errado ou
sequência antiga não mantêm uma conexão viva. Inatividade remove a conexão.

`join` é repetido a cada 500 ms enquanto necessário. Repetir o mesmo nonce no
mesmo endpoint devolve o mesmo aceite, sem criar outro jogador/conexão. O
cliente só aceita resposta para a tentativa atual; aceite atrasado não troca
uma conexão já estabelecida. Reconexão muda a tentativa, limpa remotos e
reinicia sequência da pose em zero ao receber o novo aceite.

Cada pose inclui a geração da conexão de origem. O cliente compara revisão,
geração e sequência antes de publicar o estado. Trocar a geração limpa o
histórico desse remoto. Mudar a sala/conexão local limpa todos os históricos.
Um roster da revisão atual impede uma pose atrasada de recriar um jogador que
saiu. Roster é reenviado ao mudar a composição e pelo menos uma vez por segundo.

Saída é repetida a cada 50 ms por até 200 ms. O servidor guarda até 64 recibos
temporários, permitindo repetir a confirmação sem repetir a remoção. O status
informa `leave_confirmed`; se a confirmação se perder, o timeout continua sendo
o mecanismo de limpeza. Um recibo antigo não remove uma conexão nova.

Snapshots e rosters são estado recuperável e descartável. Fatos que precisam
chegar uma vez usam o canal de eventos confiáveis abaixo, separado deles.

## Eventos confiáveis — 31/10/2026

Corpo tipado e limitado, sem texto livre, caminho ou ponteiro: `marker`
(`code` u32, marcação diagnóstica para alinhar observações entre PCs) e
`state` (`driving`, `loading` ou `unavailable`). Só estados que o cliente
determina: `loading` vem do gate de comandos; `driving`/`unavailable` vêm da
amostra de coordenadas. Não há estado de menu/pausa inferido. Contratos e
pagamentos futuros (M6) devem usar este canal com corpos próprios, sem
reaproveitar `marker`; o canal não autentica economia nem portadores.

Remetente: `stream` aleatório por processo (o nonce do join) e `id` crescente.
Até 16 eventos pendentes; acima disso, recusa local (`refused`). Um `state`
novo substitui somente estados locais pendentes antigos, contando `superseded`;
marcadores permanecem individuais. Substituição não é ACK. Reenvio em 250 ms
com backoff até 1 s, até `event_ack` ou até o
prazo (`ttl_ms` 1–30000). Pendentes sobrevivem a reconexões e são reenviados
com as credenciais novas e o mesmo `stream`/`id`.

Servidor: deduplica por identidade + `stream` (quatro streams, 128 ids
por horizonte monotônico cada), também entre reconexões e enquanto a identidade
estiver no cache. ID abaixo do horizonte é velho mesmo se nunca observado;
é confirmado sem nova entrega. Trocar/evictar o stream ou a identidade termina
essa memória: não é deduplicação econômica persistente. Cada evento novo
recebe sequência da sala e vai para a fila de cada
jogador conectado NAQUELE instante (com `--echo`, também ao remetente, como
`1000000000 + id`). Fila de 64 por destinatário, descartando o mais antigo
(`relays_dropped`); no máximo dois relays por destinatário por tick, com reenvio
em 250 ms → 1 s até `relay_ack` ou prazo (`relays_expired`). Uma retomada
herda a fila pendente. Estados são ordenados pelo id do REMETENTE no mesmo
stream antes de ganhar sequência da sala: `loading` velho recebido depois de
`driving` novo é confirmado sem reverter/retransmitir o estado (`stale_states`).
Na fila de cada destinatário, novo estado da mesma origem substitui os estados
pendentes anteriores (`relays_superseded`); não remove marcadores. O último
`state` de cada identidade é reenviado a quem
entra depois (prazo 5 s). Quem entrar depois não recebe `marker` antigo.

Destinatário: deduplica por sessão da sala + `event` (horizonte monotônico de
256 ids), entrega uma vez e confirma toda cópia. Abaixo do horizonte, só confirma:
retirar um registro não reabre sua entrega. Reordenação dentro do horizonte
continua admitida uma vez. Estado é último-ganha pela sequência da sala;
reinício da sala zera a janela. Entrega não é garantida após timeout/remoção
da conexão, após o prazo ou com fila cheia: os contadores registram cada caso.
`event_duplicates`/`relay_duplicates` incluem ids abaixo do horizonte, mesmo
quando aquela cópia específica não tinha sido observada; não estimam perda.

Compatibilidade: as quatro mensagens exigem servidor e clientes desta revisão.
Um par antigo rejeita-as como inválidas (contadas), sem afetar poses; atualize
kit/DLL/servidor juntos. A DLL publica `state` em toda sessão v2 (debounce de
300 ms, prazo 10 s, republicado a cada nova sessão da sala e a cada 3 s estáveis).
Renovação recupera um estado expirado/perdido na mesma sala sem depender de
outra mudança de estado. Ainda não é garantia de entrega sob perda ilimitada.

Estimativas de perda, sem campos novos nas poses:

- `pings_lost` (cliente): ping sem pong após 2 s ou removido da janela de oito.
  É perda de ida e volta e inclui travamentos do servidor/agendamento.
- `pose_gaps` (servidor): números de sequência pulados nas poses de subida de
  cada conexão, limitados a 4096 por salto. Perda e reordenação misturam-se
  (cópias tardias entram em `stale`); não mede a descida nem decimação.

Smoke de 31/10, binários release, loopback: dois clientes com marcadores
trocaram 17 e 12 eventos, todos com ACK e entregues uma vez. Com `udp_proxy`
de 20% de perda, 25% de duplicação e jitter no caminho do remetente, 29
marcadores chegaram exatamente uma vez; o servidor deduplicou 10 reenvios e
`pings_lost` foi 9/22. Nenhum desses números mede LAN entre PCs ou o motor.

## Tempo, coordenadas e apresentação

Posição e quaternion mantêm o formato do v1. A escala observada no jogo é cm;
o protocolo não acrescenta eixos, velocidade, reboque ou modelo não comprovados.
`client_ms` é o relógio monotônico local do remetente. `received_ms` é o instante
de recepção da pose no servidor. `server_ms`/`tick` pertencem à rede do servidor,
não ao relógio da física ou a uma captura atômica do jogo.

RTT só é medido para um ping pendente que corresponda à conexão, sequência e
instante. Há até oito pings pendentes. Estimativa de offset: instante do servidor
menos o ponto médio local, usando a amostra com menor RTT da conexão. É um
diagnóstico sujeito a assimetria de rede. Por padrão não alimenta o render (base
`arrival`). Desde 31/10, `timebase server` (opcional, local) data amostras por
`received_ms` menos esse offset; erro do offset desloca o atraso efetivo, sem
reintroduzir tremor. Ver README "Base de tempo das amostras remotas".

O diagnóstico local `network_timing` acrescenta RTT máximo, número de amostras
e EMA da variação absoluta entre RTTs consecutivos, em microssegundos. Também
mede `|delta_chegada - delta_server_ms|` dos snapshots, com EMA de peso 1/16,
sem usar offset como ajuste do render. Inclui o agendamento dos processos; não
isola atraso físico da rede. Cada conexão aceita reinicia a referência e as
métricas, que permanecem consultáveis após o leave.

Batches/duplicatas com o mesmo tick não atualizam a referência de intervalos.
Ticks/tempos regressivos contam em `nonmonotonic_snapshots` sem trocar a base.
Maior intervalo de chegada, eventos de lacuna e maior passo de tick são
observações: falta de poses e cadência também causam lacunas, portanto não são
estimativas de perda. Nenhum desses números prova suavidade, FPS ou LAN.
Não foram acrescentados campos a datagramas nem alteradas credenciais/regras.

O envio/recebimento UDP apenas publica dados Rust. Acesso a instâncias PSM,
render e liberação continuam na integração do motor e em sua thread/fase.

## Limites e observabilidade

O legado v1 também aplica o teto de 1200 bytes, 64 clientes, 240 ingressos/s
por origem conhecida e 1024/s compartilhados por desconhecidas, antes do JSON.
Não aloca uma tabela de origens desconhecidas. Broadcast usa envio não bloqueante
e tem no máximo 4096 tentativas por tick com eco. No v2, a fixture de 64 clientes
limita conservadoramente o lote a 1088 datagramas (presença + snapshots).
Ambos descartam envios que não cabem imediatamente no socket e contam erros;
nenhuma fila assíncrona cresce por destinatário. Limite de payload é precaução
contra fragmentação, sem promessa de MTU para toda rede.

O relatório final de cada servidor tem versão e `broadcast` com total/pico de
bytes e datagramas tentados e tempo de parede gasto no tick, incluindo
preparação/serialização/envio e eventual escalonamento. Respostas de controle
ficam só nos contadores gerais. Esse tempo não mede CPU isolada, motor ou FPS;
um send bem-sucedido não confirma recepção. Endurance/medição entre PCs seguem
na rodada prática acumulada.

### Extensão optativa de admissão de carreira — 30/10/2026

`Join` pode conter `admission: {"ticket":"32 dígitos hex minúsculos"}`.
Na ausência do campo, o JSON anterior permanece igual. O controle ABI v1/256
bytes de mensagem e o diagnóstico ABI v3/664 bytes não mudam. O tamanho máximo
de Options, com sala/perfil/identidade/ticket nos limites, é testado contra o
buffer existente; datagramas continuam limitados a 1200 bytes.

O host `mp_career host` compartilha um registro limitado de tickets entre seu
TCP e seu relay. A emissão exige convite/compatibilidade e uma declaração de
recibo/save-base correspondente ao manifesto. O registro associa ticket aleatório
de 128 bits, identidade, sala, perfil e manifesto/regras. Cada Join, incluindo
resume, verifica o ticket antes de criar/trocar conexão. Falhas retornam
`Rejected(reason="admission")`, terminal para o cliente desta revisão.

A sala visual sem autoridade de carreira recusa Join com ticket, evitando
fallback silencioso nesse tipo de servidor. Não enviar esta extensão a um
servidor antigo: por ter decoder estrito, ele a descarta. Clientes antigos não
interpretam a nova recusa. Usar launcher/DLL/host da revisão atual para carreira;
o fluxo sem ticket continua compatível com as salas v2 anteriores.

Até 64 leases por host, expiração monotônica de uma hora; consultar/preparar
novamente não prolonga um ticket vivo. Reconexão após expiração exige novo
ticket; conexões já aceitas continuam usando suas credenciais normais. Reiniciar
o host descarta o registro, mesmo que o manifesto permaneça idêntico. Os IDs
de sessão/conexão, tokens UDP e ticket são credenciais diferentes. O ticket
nunca aparece no status ou relatório do servidor; seu Debug é redigido.

TCP limita frames/pares/prazos como no bootstrap. Não há TLS, contas ou
atestado remoto do save carregado; portadores do convite podem reivindicar IDs.
O ticket não autoriza operações econômicas. O launcher confere arquivos locais
antes da injeção, mas isso não comprova cenário/horário/física no motor.

### Limites comuns das salas

- Até 64 conexões e 64 identidades em cache por sala; padrão 16 conexões.
- Até 240 datagramas por janela de 1 s por endpoint conhecido. Compartilhado:
  até 1024 por janela de 1 s para origens sem conexão, sem alocar por origem.
- Limitação acontece antes do parsing JSON. Janelas fixas podem permitir uma
  rajada atravessando a fronteira entre duas janelas; não são token bucket.
- Datagramas/snapshots limitados; broadcast usa envio não bloqueante. Congestão
  descarta datagramas em vez de criar backlog. A cadência efetiva é medida.
- Poses com mais de 1 s de idade não entram no broadcast. Heartbeat sem pose não
  inventa posição nem atualiza o instante de uma pose antiga.
- Relatório do servidor inclui aceites/recusas, pings/poses, invalidez, sequência
  antiga, timeout, rate limit, bytes, envios falhos e ticks atrasados.
- `SESSION_STATUS` da DLL retorna fase, IDs, reconexões, RTT/offset e confirmação
  de saída. `status=1` do worker significa thread ativa; consulte a fase para
  saber se houve aceite. Contadores de envio v2 incluem mensagens de controle.

Servidor, diagnóstico e fixtures usam apenas dados próprios. Tráfego de dois
processos no mesmo PC continua sendo loopback. Linux, jogo real, vários modelos,
sessão longa e LAN devem ter evidências registradas separadamente no roadmap.
