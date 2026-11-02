# Roadmap de mundo e save compartilhados

Data: 01/11/2026. Objetivo solicitado pelo proprietário: os participantes
jogarem a mesma carreira e reconhecerem o mesmo mundo, incluindo mapa,
texturas, caminhões, pinturas, reboques, trânsito e consequências das ações.

Este documento define trabalho futuro e critérios de entrega. Não declara
que o motor já sincroniza esses sistemas. Complementa [ROADMAP.MD](../ROADMAP.MD)
e usa [GAME_VALIDATION.md](GAME_VALIDATION.md) para registrar provas práticas.
Não substitui nem aumenta a contagem M0–M10 por haver um plano escrito.

## 1. Resultado esperado pelo jogador

O anfitrião abre uma carreira multiplayer ou importa uma cópia de uma carreira
suportada. O convidado escolhe entrar, confere a compatibilidade e recebe o
estado atual dessa sessão. Cada pessoa controla seu próprio caminhão; os
participantes veem os modelos, pinturas, cargas e ações reais uns dos outros.
Um carro do trânsito que atravessa um cruzamento para um jogador é o mesmo
carro, no mesmo trajeto, para o outro. Noite, chuva, semáforos e alterações no
mundo seguem uma decisão comum. Salvar, sair e retomar preservam esse mundo e
o progresso de cada participante.

Cada jogador conserva sua câmera, resolução, controles e interface. A meta é
igualdade do conteúdo e dos acontecimentos observados do mesmo lugar e no
mesmo instante da sessão. Não é igualdade de pixels entre monitores, GPUs ou
configurações gráficas, nem exibição instantânea sem atraso de rede. Uma
pintura deve ser a mesma; diferenças de luz/câmera/tela não significam que o
arquivo de pintura seja outro. Configurações que mudem geometria ou ocultem
obstáculos relevantes precisam de uma política de compatibilidade.

**Compatibilidade total significa cobertura de todo o estado relevante no
conjunto de versões, mapas, mods e saves expressamente suportados.** Não
significa aceitar qualquer save antigo, mod desconhecido ou arquivo corrompido.
O importador deve explicar recusas e preservar o original. Uma limitação
temporária é informada como tal; não encerra o objetivo final.

## 2. Ponto de partida comprovado

| Área | Já existe | Ainda falta para este objetivo |
| --- | --- | --- |
| Presença dos jogadores | Posição/orientação e visual mútuo na primeira LAN Windows↔Fedora; provas locais posteriores | Aparência real por jogador, estados adicionais e nova matriz LAN |
| Visual remoto | Modelo e shader de teste fixos; textura e iluminação confirmadas | Modelo/cabine/pintura/reboque próprios; materiais independentes |
| Movimento | Buffer, interpolação/previsão, limites e bancada de rede | Medição ampla no jogo/LAN, rodas/articulações e física comum |
| Save inicial | `mp_career` transfere cópia congelada com manifesto, hashes e recibo | Instalação/carregamento confirmado no motor e entrada no mundo atual |
| Compatibilidade | Comparação declarada de perfil/mapa/mods; novo inventário offline de camadas/recursos em ordem declarada | Conteúdo/ordem efetivamente montados e versão efetivamente carregada |
| Carreira | Ledger, relógio, checkpoints e recuperação próprios, fora do motor | Aplicação ao horário, veículos, contratos e economia reais do jogo |
| Trânsito/ambiente | Cada jogo mantém sua simulação | Autoridade única e aplicação remota comprovadas |
| Proteção | Backup, limpeza e verificações já entregues; M2 9/9 | Ampliar proteção aos novos adaptadores e ao salvamento conjunto |

Base de referência do código: `505e63f`, após o lote de relógio de M5.
G0/G12/G13 e a LAN histórica preservam exatamente seus escopos: eco/fixture
não equivalem a trânsito sincronizado ou nova prova entre computadores.

## 3. Regras de arquitetura deste plano

1. **Uma decisão por estado compartilhado.** O serviço de sessão ordena e
   persiste decisões. Inicialmente, o jogo do anfitrião é o candidato a
   simular o ambiente/trânsito; precisa dos adaptadores comprovados abaixo.
   Cada caminhão de jogador começa com física local e envio de estado.
   Colisão compartilhada exige contrato adicional, não decorre desse envio.
2. **Servidor e jogo continuam separados.** O relay/serviço Rust não ganha a
   física original por receber coordenadas. O papel de simulador do ambiente
   fica explícito e substituível, sem exigir GPU ou instalação no relay.
   Hospedagem externa permanece futura; sair do anfitrião inicialmente
   suspende/encerra a sessão de maneira coerente. Não há migração automática
   prometida nem eleição de novo simulador sem protocolo específico.
3. **Conteúdo local idêntico; estado dinâmico transmitido.** Mapa, modelos e
   texturas têm identificação/verificação e resolução local. Movimento,
   mudanças de aparência, spawn e regras trafegam como dados tipados.
   Nunca aceitar caminhos de arquivo, ponteiros ou código vindos de um peer.
4. **Save-base não é estado vivo.** A cópia inicial é uma base imutável.
   Checkpoint de sessão, alterações posteriores e estados de jogadores têm
   revisões próprias. Não copiar continuamente um `.sav` enquanto o jogo o
   escreve, nem fazer os dois processos escreverem o mesmo arquivo de rede.
5. **Sem duas simulações disputando o mesmo objeto.** Um veículo remoto não
   pode ter simultaneamente IA local e controle remoto. Desabilitar a IA
   original seletivamente e tratar seus colliders são descobertas obrigatórias.
   Usar a mesma semente aleatória não comprova simulação determinística.
6. **Integração segue as proteções atuais.** Descobrir leitura, ABI, thread,
   ownership e teardown antes de ampliar chamadas do motor; aplicar pela
   fila validada. A thread UDP não manipula objetos do motor diretamente.
   Não forçar descarga da DLL fixada nem reutilizar ponteiros após uma carga.

### Divisão entre mundo comum e dados de cada jogador

| Estado | Responsável planejado | Regra |
| --- | --- | --- |
| Versão, mapa, ordem de mods, regras e sessão | Serviço de sessão | Identidade/revisão fixadas na admissão |
| Caminhão controlado pelo jogador | Dono, com validação da sessão | Estado/visual publicados; consequências comuns confirmadas separadamente |
| Trânsito, semáforos e eventos ambientais | Simulador designado, inicialmente anfitrião | Uma geração de autoridade; clientes não tomam decisões paralelas |
| Relógio, clima e pausa global | Serviço ordena; simulador aplica | Mudança versionada e confirmada pelo motor |
| Contratos, empresas, dinheiro e entregas | Autoridade de carreira | Operações persistentes com deduplicação, ligadas a evidência do motor |
| Save/checkpoint conjunto | Coordenador da carreira | Corte consistente e recuperação verificável |
| Câmera, idioma, teclas, volume e preferências | Cada PC | Não alteram o estado comum |
| Posição, caminhão, carga e progresso individual | Carreira, por identidade durável | Compartilham a sessão sem clonar a identidade do anfitrião |

## 4. Ordem e dependências

Os identificadores W abaixo acompanham este objetivo; não são marcos M novos.
Os critérios amplos continuam pendentes. As peças já existentes da seção 2 serão reutilizadas,
sem marcar uma etapa inteira por uma entrega parcial anterior.

| Etapa | Entrega | Depende de | Relação com o roadmap principal |
| --- | --- | --- | --- |
| W0 | Inventário do estado e contratos de compatibilidade | Base atual | M0/M1/M6/M7 |
| W1 | Conferência do conteúdo e do mundo carregado | W0 | M1/M3/M6/M7/M9 |
| W2 | Entrada segura na carreira e separação de jogadores | W0/W1 | M2/M6/M8/M9 |
| W3 | Caminhão e pintura reais de cada jogador | W0/W1; W2 para carreira | M1/M4/M5/M7 |
| W4 | Reboques, cargas e sinais visuais | W3 | M1/M4/M5/M6 |
| W5 | Entrada tardia e recuperação do mundo atual | W2; cresce com W3–W10 | M3/M4/M6/M8 |
| W6 | Relógio, clima, pausa e transições comuns | W2/W5 | M1/M2/M6 |
| W7 | Trânsito e objetos dinâmicos sob autoridade única | W1/W5/W6; W3/W4 para visual | M1/M3/M4/M6 |
| W8 | Interações, colisões e efeitos coerentes | W3/W4/W7 | M4/M5/M6/M8 |
| W9 | Economia, fretes e progresso ligados ao jogo | W2/W4/W5; W8 para danos | M1/M6/M9 |
| W10 | Salvar e retomar o mundo conjunto | W2/W5; contratos dos sistemas persistidos | M2/M6/M8/M9 |
| W11 | Duração, escala e compatibilidade entre PCs | Incremental; fechamento após W1–W10 | M3/M5/M7/M8/M9 |
| W12 | Fluxo de uso e aceite do mundo compartilhado | W1–W11 | M8/M9 |

W3 é a primeira prioridade visual após definir W0/W1. W2 e W5 não precisam
esperar toda a aparência para avançar offline. W5 e W10 recebem uma base cedo
e são ampliados a cada sistema: não se adia a persistência até existir trânsito,
nem se declara snapshot completo antes de incluir o trânsito. A validação
acompanha cada entrega; W11 não é o primeiro momento de testar robustez.

## 5. Etapas e critérios de conclusão

### W0 — Inventário e contrato do mundo suportado

- [ ] Enumerar estado de save e de memória: mundo estático/dinâmico, jogador,
  veículo, carga, economia, tempo, clima e scripts/gatilhos. Para cada campo,
  registrar unidade, identidade, fonte, persistência e responsável.
- [ ] Classificar cada campo em compartilhado, individual, cosmético local ou
  ainda desconhecido. Campo desconhecido relevante impede afirmar compatibilidade
  total; não pode ser descartado silenciosamente na importação.
- [ ] Criar matriz de capacidades: ler/aplicar/criar/remover/salvar/restaurar,
  por perfil de executável e sistema. Separar evidência estática, fixture e jogo.
- [ ] Definir schemas versionados, limites e política de migração/recusa para
  carreira, conteúdo, jogador, objeto e snapshot; reaproveitar os contratos atuais.
- [ ] Registrar decisão de autoridade e política inicial de pausa/desconexão;
  parâmetros de precisão/frequência só são aprovados com medição posterior.

**Aceite:** nenhum sistema pedido fica sem responsável ou pendência nomeada.
Catálogo de fixture permite exercitar importação/comparação sem assets comerciais.

### W1 — Mesmo mapa, modelos e texturas efetivos

- [ ] Conferir executáveis/perfil, revisão do mod e capacidades. Separar versão
  declarada de hash do arquivo observado e de módulo realmente carregado.
- [ ] Inventariar arquivos/pacotes e a ordem efetiva de montagem: mapa/estradas,
  colisão, definições, modelos, pinturas, texturas, materiais e dependências.
  O conjunto ordenado por hash existente não descreve prioridade de overrides.
- [ ] Resolver conflitos por recurso: dois pacotes iguais em ordem diferente
  não são aceitos automaticamente; registrar o conteúdo vencedor e sua origem
  local privada. Caminhos particulares não entram no relatório compartilhável.
- [ ] Separar alterações cosméticas locais permitidas (por exemplo, interface)
  das que mudam o mundo. Recurso visual de mundo divergente impede o modo estrito.
- [ ] Conferir o mapa/versão efetivamente carregados e alterações de montagem
  depois do preflight; invalidar admissão se o mundo mudar. Sem leitura comprovada
  do motor, informar que a conferência é apenas de disco e manter o gate pendente.
- [ ] Explicar incompatibilidade antes da entrada; preparar correção local
  reversível. Distribuir somente conteúdo próprio/autorizado; cada PC usa seus
  originais. Pacotes derivados continuam privados, sem envio implícito pela rede.

**Aceite:** mapa errado, pintura ausente e ordem conflitante são detectados;
os dois PCs resolvem os mesmos recursos numa instalação suportada. A verificação
de arquivos não é anunciada como prova de carregamento ou assinatura de segurança.

### W2 — Carreira comum sem clonar o jogador do host

- [ ] Reaproveitar transferência de save-base, manifesto, recibo, plano e ticket;
  mapear o que já passam a conferir e o que ainda não sabem sobre o motor.
- [ ] Importar save existente para NOVA carreira privada após verificar o formato
  e a matriz suportada. Preservar o original; migração incompatível é recusada
  com motivo, inclusive mods/campos desconhecidos que afetem o estado comum.
- [ ] Separar mundo da identidade/empresa/caminhão/carga de cada participante.
  Não atribuir saldo, contrato ou veículo do host ao convidado por copiar o save.
- [ ] Preparar instalação/restauração e carregamento controlado com jogo fechado
  quando arquivos exigirem isso; confirmar dentro do motor a carreira/revisão
  carregada antes de liberar spawn e controle multiplayer.
- [ ] Resolver spawn seguro, retorno de jogador conhecido e jogador novo;
  cada caminhão tem identidade própria sem sobreposição/duplicação.
- [ ] Preservar `Become a Millionaire`, meta/sem limite e regras de carreira;
  adaptar a condição real de vitória antes de afirmar que a regra está ativa.

**Aceite:** convidado parte da carreira correta com identidade e caminhão
próprios; save errado/antigo não entra silenciosamente. Save normal permanece
restaurável. Ainda não fecha sincronização do mundo durante a partida.

### W3 — Modelo, cabine e pintura corretos

- [ ] Descobrir por leitura as referências do caminhão local: chassi/modelo,
  cabine, pintura/cor/material, acessórios e variantes existentes. Cor pode
  vir da textura/material; não presumir um único campo RGB.
- [ ] Criar descritor de aparência com IDs do catálogo validado, revisão e
  dependências. Vincular ao jogador/veículo/geração, sem paths recebidos da rede.
- [ ] Transportar a aparência de forma confiável, incluindo troca em oficina,
  entrada tardia e reenvio após reconexão. Pose só referencia revisão conhecida.
- [ ] Ampliar criação/draw/liberação somente aos contratos comprovados do motor;
  materiais e instâncias de cada remoto não podem modificar os do jogador local.
  A restrição atual a modelo/shader fixos só sai após essa prova.
- [ ] Resolver ausência/incompatibilidade antes da entrada no modo estrito;
  fallback de diagnóstico pode existir, mas não conta como aparência correta.
- [ ] Cache limitado, troca sem referência pendurada, limpeza após falha parcial
  e recriação com a aparência correta após mapa/save/reconexão.

**Aceite:** dois caminhões de modelos e pinturas diferentes aparecem corretamente
nos dois sentidos; trocar a pintura de um não muda a do outro. Confirmar de dia
e à noite com as mesmas condições ambientais e evidência de recursos resolvidos.

### W4 — Reboques, carga e detalhes em movimento

- [ ] Identificar modelo/pintura/eixos de reboque e representação da carga;
  definir IDs e relação proprietário↔caminhão↔reboque↔carga.
- [ ] Capturar articulação/transform do reboque; não inferir tudo pela pose do
  cavalo. Representar anexar/desanexar de forma confirmada e idempotente.
- [ ] Publicar faróis, lanternas/freio, setas/pisca-alerta, direção/rodas e
  demais animações existentes no inventário W0, com unidades/limites conferidos.
- [ ] Tratar mudança de veículo, troca/perda de carga, oficina, reboque solto
  e recarga sem peças antigas flutuando ou relações duplicadas.
- [ ] Separar estado discreto confiável de animação interpolável; som posicional
  de motor/buzina segue a mesma origem quando comprovado, com volume local.

**Aceite:** curva/ré com reboque, engate/desengate e sinais aparecem coerentes
dos dois lados; reinício/reconexão preservam identidade e relação da carga.

### W5 — Snapshot completo, entrada tardia e ressíncronia

- [ ] Definir IDs estáveis de carreira/objeto e gerações de mundo/autoridade;
  não usar endereço de memória ou ID transitório do relay como identidade durável.
- [ ] Criar snapshot versionado de mundo+jogadores e um corte de sequência para
  alterações posteriores. Quem entra depois recebe o estado atual, não só o save-base.
- [ ] Separar canais: poses descartáveis, mudanças confiáveis e transferência
  limitada de snapshots. Medir orçamento; não colocar o mundo num datagrama1200B
  nem reaproveitar o wire v2 mudando sua semântica silenciosamente.
- [ ] Receber/verificar/preparar/aplicar/confirmar antes de marcar jogador pronto;
  enfileirar deltas limitados desde o corte ou pedir snapshot novo em overflow.
  Comandos que afetam o mundo ficam bloqueados até a aplicação terminar.
- [ ] Recuperar perda/reordenação/duplicatas/snapshot interrompido, objeto cuja
  aparência não chegou e pacote de geração antiga. Comparar resumos sem exigir
  igualdade binária dos buffers interpolados em instantes locais diferentes.
- [ ] Reentrada com identidade conhecida retoma exatamente um jogador; perda do
  host usa a política explícita de suspensão/encerramento, sem duas autoridades.

**Aceite:** convidado entra após host dirigir, trocar pintura, hora ou carga,
e recebe a revisão atual. Replay/ACK perdido não duplica objeto ou ação.

### W6 — Horário, clima, pausa e transições

- [ ] Descobrir leitura/aplicação do relógio do jogo, escala temporal e ambiente;
  o relógio existente do sidecar não é prova de alteração no motor.
- [ ] Autoridade define hora/clima e suas transições; clientes aplicam revisões
  com correção limitada e política explícita para grandes desvios.
- [ ] Definir pausa: menu de um cliente não congela o mundo dos demais; pausa
  global, dormir, viagem, oficina/resgate e avanço rápido exigem decisão comum
  e barreira de retomada. A viabilidade de manter o cliente ativo sem foco é verificada.
- [ ] Integrar carga/mapa/saída com gates e readiness; nenhum cliente continua
  no mundo antigo enquanto recebe objetos da geração nova.
- [ ] Sincronizar estados existentes de iluminação ambiental, visibilidade,
  chuva/neblina e efeitos relevantes conforme o inventário. Efeitos puramente
  cosméticos só podem variar se não ocultarem estado/obstáculos comuns.

**Aceite:** dia/noite, chuva e transição/retomada coincidem nos dois jogos;
menu, perda de foco e retorno não criam um mundo local independente.

### W7 — O mesmo trânsito e os mesmos objetos dinâmicos

- [ ] Descobrir ciclo completo da IA: criação, decisão, rota, transform,
  remoção e colisão; mapear carro/caminhão/ônibus e outros tipos realmente
  presentes. Incluir trens, polícia, pedestres, portões ou eventos se existirem
  e forem relevantes, sem presumir suporte desses sistemas.
- [ ] Comprovar como impedir spawn/decisão autônoma duplicada nos clientes,
  preservando o jogador e os visuais remotos. Inventariar colliders invisíveis.
  Não enviar comando desconhecido ao motor por hipótese.
- [ ] Designar único simulador por geração, inicialmente o host; catálogo,
  tipo/pintura, spawn/despawn, rota/estado e transform seguem IDs globais.
- [ ] Replicar semáforos e outros controladores de tráfego como estados comuns;
  mesma seed ou mesma hora inicial não bastam para provar convergência.
- [ ] Tratar regiões de interesse: jogadores em cidades diferentes precisam de
  tráfego autoritativo nas duas áreas. A região carregada pelo host não define
  sozinha todo o mundo. Investigar streaming, simulação de regiões ou autoridade
  por região com handoff; sem contrato comprovado, distância limitada é entrega
  intermediária explícita, não W7 concluída.
- [ ] Separar ocultação/LOD local de destruição global; carro que sai da câmera
  não deve desaparecer para quem ainda está perto. Handoff não cria duas IAs.
- [ ] Replay, reconexão e load removem referências e veículos obsoletos;
  medir custo e densidade antes de ampliar quantidade/região.

**Aceite:** os mesmos veículos com cores/rotas corretas atravessam um cruzamento
nos dois PCs; semáforos concordam e não há trânsito local extra. Cobrir observação
em sentidos opostos, separação entre cidades, reencontro e entrada tardia.

### W8 — Física, colisões e consequências comuns

- [ ] Mapear interação jogador↔jogador, jogador↔trânsito, reboque↔mundo e
  obstáculos, incluindo origem de dano, multas, impulsos e contatos.
- [ ] Escolher e documentar autoridade física viável após descobrir o contrato:
  física do host, ownership com validação/correção ou solução híbrida explícita.
  O relay não simula o motor; duas físicas locais soltas não fecham este marco.
- [ ] Replicar colliders/interações somente com operações comprovadas e uma
  decisão de consequência. Visual sem colisão é etapa anterior identificada.
- [ ] Definir previsão/reconciliação/teleporte seguro, limites de correção e
  proteção contra aplicar duas vezes impacto, dano ou multa.
- [ ] Resolver desconexão/pausa no contato, veículo parado em rota, resgate e
  reboque solto. Política de remoção deve evitar obstáculos fantasmas.

**Aceite:** contato observado de ambos os lados produz resultado compatível,
sem um PC atravessar um obstáculo sólido que existe apenas no outro. Se o motor
não permitir um contrato seguro, registrar bloqueio técnico; não marcar como
concluído só por desligar colisões ou esconder o objeto divergente.

### W9 — A mesma carreira em andamento

- [ ] Adaptar empresa/veículo/carga/contrato do motor ao ledger existente,
  preservando progresso individual e identificadores duráveis da carreira.
- [ ] Capturar oferta/aceite/carregamento/transporte/entrega com evidência
  verificável; substituir atestado manual de bancada por integração comprovada.
- [ ] Garantir aceite exclusivo e pagamento único com IDs de operação, inclusive
  após perda de ACK, save/reload ou retorno do mesmo jogador.
- [ ] Sincronizar compra/venda/troca/oficina/combustível/danos/multas/empréstimos
  e demais sistemas econômicos identificados em W0. Evitar débito/crédito
  duplicado pela execução simultânea da regra original e da regra do mod.
- [ ] Aplicar saldo, carga, regras de meta/sem limite e condições de término
  no motor; apresentação de moeda/idioma não altera valores da carreira.
- [ ] Rotas/objetivos individuais continuam identificados por jogador;
  um comboio pode compartilhar rota e marcadores aprovados, sem substituir
  silenciosamente o destino do contrato de outro participante.

**Aceite:** aceitar→transportar→entregar→receber funciona na partida e só uma
vez; os participantes consultam o mesmo estado comum e seu progresso correto.

### W10 — Save conjunto e recuperação

- [ ] Definir pacote de sessão: base compatível imutável, versão/manifesto,
  revisão/epoch, mundo persistente, checkpoints individuais e journal de ações.
  Definir explicitamente o estado efêmero reconstruído (por exemplo trânsito)
  e como a reconstrução chega idêntica aos participantes após retomar.
- [ ] Salvar por barreira/corte coerente com confirmação do motor e sem perder
  operações durante a escrita. Se não houver snapshot atômico, quiescer sistemas
  e registrar a janela real; não combinar saldo de uma revisão com carga de outra.
- [ ] Persistir em destino novo/temporário, verificar integridade e publicar
  revisão recuperável com estratégia comprovada no Windows/Fedora. Testar queda
  nos pontos críticos; rename/sync isolados não prometem durabilidade universal.
- [ ] Ao retomar, recuperar a última revisão completa, reaplicar apenas operações
  confirmadas e fazer os clientes convergirem antes de liberar controle.
- [ ] Migrar schemas e validar conteúdo/perfil novamente; recusas preservam a
  revisão antiga e o save de uso normal. Criar restauração demonstrável.
- [ ] Definir saída do convidado e do host, conexão perdida durante save,
  gravação automática e política de rollback comum. Save local arbitrário
  não pode substituir a carreira do servidor nem duplicar progresso.

**Aceite:** fechar e reabrir nos dois PCs recupera jogadores, aparência, carga,
economia, ambiente e objetos persistentes corretos. Gravação interrompida
recupera revisão completa sem mistura; trânsito reconstruído é distribuído
pelo responsável, não sorteado independentemente em cada PC.

### W11 — Robustez, diferenças de plataforma e custo

- [ ] Testar codecs/limites/estados com fixtures próprias e falhas de rede,
  ordem, geração, autoridade e persistência; diagnóstico identifica o subsistema
  divergente sem exportar saves, tokens ou paths privados por padrão.
- [ ] Medir CPU/memória/rede/FPS e quantidade de objetos em jogo com dois PCs,
  depois mais participantes. Usar cadências e budgets medidos; 60 Hz de relay
  não obriga enviar pintura/mapa/clima 60 vezes por segundo.
- [ ] Rodar 30–60 min ou mais conforme o cenário, com regiões distintas,
  entrada/saída/recarga/reconexão, clima e crescimento do mundo. Comparar recursos
  após ciclos equivalentes, distinguindo cache de vazamento.
- [ ] Conferir Windows e Fedora/Proton, instalação limpa do kit e APIs/perfis
  suportados. UI local/driver não justificam conteúdo ou física diferentes.
- [ ] Exercitar falhas de snapshot/checkpoint/asset: nenhum parcial libera
  controle em estado divergente. Recuperação, recusa ou suspensão tem resultado
  explícito; não marcar sucesso com objeto de teste substituindo o real.

**Aceite:** orçamento e tolerâncias documentados com dados do motor e da LAN;
divergência detectada leva a recuperação ou parada coerente, sem corromper save.

### W12 — Uso simples e aceite final

- [ ] Fluxo anfitrião: escolher/importar carreira, conferir conteúdo, abrir,
  convidar, acompanhar estado, salvar e encerrar com resultado compreensível.
- [ ] Fluxo convidado: entrar, ver o que falta/corrigir de forma reversível,
  aguardar mundo pronto, controlar seu veículo e retomar após desconectar.
- [ ] Exibir modo e limitações reais; não chamar modo visual limitado de
  mundo integral. Status diferencia arquivo recebido, motor carregado,
  snapshot aplicado e sistemas ainda não sincronizados.
- [ ] Kit/manifestos de versão/diagnóstico/restauração funcionam fora do
  repositório. Sem comandos manuais de PID/ponteiro para o uso normal.
- [ ] Concluir o aceite cruzado descrito abaixo, ligando cada evidência à
  revisão do código, conteúdo, checkpoint e sistemas exercitados.

**Aceite:** os participantes entram na mesma carreira, reconhecem os mesmos
objetos/acontecimentos, interagem e retomam sem preparar saves manualmente a
cada vez; todas as limitações restantes estão fora do escopo declarado suportado.

## 6. Validação em lotes, com pouca intervenção manual

Uma rodada pode cobrir vários critérios; não pode comprovar um sistema que
ainda não existe. Não prometer um único passeio que valide universalmente
saves, mods, física e todos os estados do motor. Reusar G0/G12/G13 para seus
critérios já observados, logs para regressões offline e novas provas somente
para integrações concretas ou lacunas. A sessão final integra os resultados.

| Lote | Pode ser feito sem outro PC? | Roteiro e evidência nova |
| --- | --- | --- |
| A — Contratos/conteúdo/save | Sim, offline | Fixtures de importação, ordem de pacotes, aparência, snapshot/corte, replay, save interrompido e migração; disco não prova montagem no motor |
| B — Adaptadores visuais e de carreira | Um desktop com jogo, quando entregues | Modelo/pintura/reboque/sinais, troca em oficina, câmera, load e limpeza na mesma abertura; agrupar regressões G2/menus M9 pertinentes |
| C — Mundo vivo e retomada | Um desktop prova partes; fechamento usa dois | Relógio/clima, controle de IA, semáforos, snapshot tardio e salvar/reabrir; peer de fixture não comprova motor remoto |
| D — Paridade cruzada | Dois PCs | Sem eco, mesma área: dois modelos/pinturas, engate, sinais, cruzamento/IA, noite/chuva; depois cidades separadas, reencontro e entrada tardia |
| E — Interações/persistência/endurance | Dois PCs; falhas offline antes | Colisão/consequências, contrato/pagamento, reconexão, save/retomada e duração; falha abrupta só em ambiente de teste preparado |

Antes de cada prova no motor, registrar **quais critérios novos o build pode
validar**, resultados esperados e duração estimada. Reunir mudanças compatíveis
num roteiro; não instalar novo build com DLL residente. Pedidos de movimento
ou menu só quando necessários para uma descoberta/prova determinada.

### Registro mínimo de evidência por critério

- Etapa/item, revisão do código e perfis/capacidades; modo visual ou integral.
- Fingerprints de conteúdo/ordem, carreira/checkpoint/epoch e identidade do
  cenário de teste; dados privados completos ficam fora do Git.
- Tipo de prova: projeto, fixture/offline, motor em um PC, LAN entre PCs ou
  endurance. Resultado humano separado de contadores e checks automáticos.
- Para conteúdo: hash/resolução e comprovação de montagem. Para estado discreto:
  revisão/ordem/recibo. Para movimento: tempo de referência, divergência de
  posição/orientação e tolerância medida, não comparação de frames aleatórios.
- Limites/erros observados e recuperação; sem converter ausência de log em
  ausência de falha. Integridade/restauração do que foi alterado e limpeza final.

## 7. Riscos que podem impedir a promessa integral

| Descoberta necessária | Se não for possível no motor |
| --- | --- |
| Ler/aplicar aparência sem alterar materiais de terceiros | W3 não fecha; manter modelo de teste identificado |
| Separar mundo e estado individual do save | W2/W10 não fecham importação integral; restringir formatos e explicar |
| Confirmar conteúdo efetivamente carregado | W1 continua apenas preflight de disco |
| Suprimir IA/colliders locais e criar equivalentes remotos | W7/W8 não fecham; não chamar trânsito local de compartilhado |
| Simular regiões longe do host | Mundo integral fica bloqueado; proximidade é escopo intermediário |
| Aplicar horário/clima e transições de forma segura | W6 não fecha por existir clock no sidecar |
| Autoridade física e reconciliação viáveis | Visual sem colisão permanece intermediário |
| Checkpoint consistente com recuperação | Não anunciar save conjunto confiável por transferir um `.sav` |

Descoberta negativa pode exigir outra arquitetura; registrar custo/alternativas
antes de ampliar unsafe. Não inventar contrato de API ou prazo para contornar
o bloqueio. Internet, migração automática de host e suporte universal a mods
não são requisitos escondidos para a primeira entrega LAN deste plano.

## 8. Próximo bloco concreto

### Entrega offline W1 inicial — 01/11

`world_content` fornece schema1 de camadas, recursos por hash, resolução
de conflitos por prioridade declarada e comparação estrita. `asset_tools
world-content` lê ZIP/SCS locais, sem extrair, grava manifesto novo e omite
nomes/paths; `world-content-compare` retorna falha em divergência. Mesmo conjunto
legado de mods pode ter vencedores diferentes se a ordem muda. Projeção para
`career::Compatibility` serve só à comparação legada, sem transformar a
admissão existente em prova de ordem/conteúdo carregado.

Quatro testes do contrato e quatro do scanner nativo passaram, incluindo
reordenação, relatório limitado, aliases, link e entrada comprimida acima do
limite recusada antes da expansão. CLI real com três arquivos próprios
detectou o conflito de uma pintura e aceitou conteúdo idêntico. Sem assets do
jogo, prova x86 nova ou montagem observada nesta entrega. Comandos/limites
estão no README; o arquivo de fingerprints é privado por padrão.
W0/W1 não ficam completos: inventário do estado de save, capacidades e
verificação de montagem no motor continuam pendentes.

### Sequência de implementação

Base W3 offline de01/11: catálogo/descritor versionados vinculam modelo/pintura
e slots declarados aos recursos conhecidos do manifesto. Estado puro limitado
recusa dono duplicado, geração/revisão antiga e conflito na mesma revisão;
pose exige revisão exata. `mp_career catalogue-check`/`appearance-check` fazem
o preflight offline, sem aplicar o resultado no motor ou usar o relay.
Seis testes nativos e CLI com arquivos próprios passaram (aceite e modelo
desconhecido recusado). Ainda faltam descoberta do modelo/material local,
tipo/ABI dos recursos, captura, transporte real e integração de desenho.
Nenhum item amplo de W3 foi encerrado por esse contrato de bancada.
Catálogo schema2 de composição (01/11) acrescenta peças, pais e materiais
por peça, com plano pai→filho e resolução de recursos. Posição desconhecida ou
offset bruto continuam identificados; declaração local não comprova o motor.
Schema1/hash anterior preservados, sem alteração da aparência real no jogo.
Leitura estática local adicional identificou composição por acessórios com
cabine/base em modelos separados, listas de shader e variantes de pintura.
São candidatos, não captura/aplicação comprovada. O catálogo inicial é de
referências declaradas; montagem de múltiplas partes e relação material/parte
precisam ser ampliadas após confirmar o contrato efetivo do motor.

Auditor estático W3 de01/11: `asset_tools vehicle-references` vincula leitura
de definições ao inventário por hashes de arquivos e recursos. Mantém handles
abertos e confere arquivos novamente antes da saída; não é snapshot atômico
nem sandbox contra escritores concorrentes. Percorre até128 definições de64KiB,
sem extrair/copiar conteúdo original. Lista somente chaves/hashes, campos e
candidatos; interpreta lexicalmente `accessN`, `extdefs`, `model`, `shader` e
rótulo `type`, sem aplicar herança ou converter offsets. Busca relativa na
pasta da definição/compartilhada do veículo é hipótese explícita, não regra
observada do motor; ambiguidades e falhas de lexing ficam parciais. Relatório
novo até8MiB, privado por padrão, flags engine/worldfalse. Não gera catálogo
aplicável automaticamente. Leitura local de mch613 retornou20 definições,
96 referências, uma definição não lexável e três referências não suportadas;
zero candidatos ausentes/ambíguos nessa busca estreita não comprova completude.
Casos próprios cobrem ambiguidades, importação cíclica deduplicada, raiz VFS,
arquivo alterado/limites/aliases e6kmutações do lexer. W3 permanece parcial.

Transporte W3/W5 optativo de01/11: o host da carreira pode habilitar fluxo de
descritores declarados por três arquivos content/catalogue/seed. Reaproveita
TCP e Registry de tickets; publicações só do principal, com hashpins/gerações,
sequência, retry idempotente, páginas e snapshot para leitor atrasado. CLIs
pull/publish/remove verificam respostas, sem aplicar no motor. Seed128KiB,
history128/page32, budgets64/s global8/s ID. O sidecar do seed é CONGELADO,
não o ledger/clock atuais. Reinício requer geração nova definida pelo operador,
sem persistência/guard automático de geração/head; novo ticket obrigatório.
Cliente RPC é stateless e não mantém pisos de Receiver entre chamadas.
Scope continua sidecar_appearance_only/flagsfalse. Critérios amplos de W3/W5
seguem abertos: falta captura, render por instância e readiness integrado.
Bench TCP próprio/codec/CLI não substitui o aceite B/D entre dois PCs.

Leitor W3 de candidatos de01/11: análise estática identificou make+0x1770 e
nomes com stride20/+0xd9d/countbyte+0x300; truck_build vazio antigo continua
descartado como fonte suficiente. `memory_tool appearance` faz somente leitura
com perfil/registro/vida/módulos e duas cópias/testemunhos de cadeia/owner/count.
Campos ainda não foram conferidos numa partida real; não é tick/ABA/teardown
universal. Saída de hashes candidate_only, sem poses/ponteiros/textos e sem
publish automático. `appearance-references` liga esses hashes a arquivos
conferidos pelo inventário, sem extrair, e separa slots de defaults/importações.
Disponibilidade local não comprova conteúdo montado ou seleção/material/cor
renderizados. Sem novo ABI/draw/descritor automático, W3 continua parcial.

Consulta W3 de recursos (02/11) usa três campos PSM identificados estaticamente
e produz chaves candidatas de modelo/material, com ordem de tabela preservada
e ausência explícita. Leitura externa com perfil, limites, tipo/packet e cópias
repetidas; testes com memória própria não validam esses campos na partida.
Não abrange todos os acessórios nem prova pintura, formato, montagem ou cor.
O próximo vínculo offline é com o inventário; aplicação por instância/ownership
e confirmação visual continuam para B/D, sem afrouxar o guard do modelo de teste.
O vínculo offline agora existe em `asset_tools appearance-resources`: confere
bytes pelos hashes do inventário, mantém os três campos, faltas e repetições,
sem default ou montagem automática. Formatos, conteúdo montado, composição
completa, cor e aplicação por instância continuam não verificados.
Preparo B/D de02/11: registros tipados de aparência podem ser coletados junto
dos status habituais e exportados separadamente dos logs de conexão. Preservam
recusas/divergências, hashes de preflight e ordem nominal local, sem afirmar
simultaneidade ou sincronização. Comparação/extração offline reutiliza os
auditores existentes, reduzindo repetição de teste somente por falta de coleta.

Base W5 offline: `world_join` prepara um snapshot tipado do sidecar existente
e aparências declaradas, com corte/fluxo exclusivo de aparência, sem duplicar
o formato do ledger. Vincula carreira/conteúdo/catálogo/mundo/autoridade;
rejeita retrocesso e mudança de conteúdo no mesmo corte/checkpoint observado.
Buffer128/evento3KiB/snapshot1MiB; lacuna/overflow/conflito invalidam preparo
e exigem nova cópia cobrindo a sequência observada. Preparação constrói estado
temporário completo; uma falha não retorna quadro parcialmente pronto.
Gerações removidas ficam aposentadas em até128 registros preservados no snapshot.
Isso não é snapshot completo do mundo: trânsito, física, relógio do motor,
estado desconhecido do save e aplicação real continuam ausentes. O escopo é
`sidecar_appearance_only` e as flags engine/world permanecem false.

Nove testes W5, sete do estado de aparência (inclui aposentadoria) e a suíte
nativa do servidor com83 testes passaram; o mutador exerceu12.000 entradas
próprias nos dois decoders. CLI própria aceitou corte10+evento11 e recusou epoch
incorreto. O parser do snapshot econômico agora recusa contratos duplicados
antes de substituir silenciosamente uma entrada. Verificação integral do lote
é registrada no ROADMAP; nenhum item amplo W2/W5/W10 foi encerrado.
Fechamento conjunto:321 testes x86 Proton/389nativos/releases/Clippy/fmt;
CLI x86 de catálogo/preparo/scanner próprio passou e manifesto foi idêntico
ao nativo. Sem nova partida/LAN; detalhes e limites no registro do ROADMAP.

1. **W0 + base W1, offline:** catálogo tipado de estado/capacidades e manifesto
   de conteúdo efetivo planejado, incluindo ordem/conflitos, migração dos
   fingerprints existentes e comparação com fixtures próprias. Campos não
   observados no motor permanecem explicitamente não verificados.
2. **W3, descoberta por leitura:** aproveitar os leitores de memória/metadata
   existentes para identificar modelo/material/pintura do veículo local.
   Preparar parser/descritor/replicação e fixtures antes de pedir sessão humana.
   Qualquer chamada nova para aplicar aparência depende do contrato comprovado.
3. **Base W2/W5/W10, offline:** inventário do save e snapshot/checkpoint comum
   reutilizando bootstrap/ledger/journal, sem outro formato paralelo de carreira.
   Compatibilidade e ciclo de vida vêm antes de ampliar trânsito e colisões.

Os incrementos acima preparam esse plano sem modificar motor, saves,
instalação ou kits. Os critérios amplos exigem as provas indicadas por etapa.
