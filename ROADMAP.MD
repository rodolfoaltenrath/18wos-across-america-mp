# Roadmap — Across America multiplayer em Rust

Última atualização: 27/10/2026. Este arquivo descreve o projeto, não uma promessa
de prazo. Checkboxes concluídos exigem implementação ou evidência registrada.
O objetivo é desenvolver ferramentas de assets e multiplayer para a instalação
original Steam de **18 Wheels of Steel: Across America**, com todo o código e
automação próprios em **Rust**. O jogo original permanece instalado separadamente
e não será distribuído pelo projeto.

**Ambiente de continuidade informado pelo proprietário: Linux.** Ferramentas e
servidor nativos e DLL para Windows x86 têm fluxos de compilação diferentes.
A seção 8 contém o procedimento Linux. Os testes de integração registrados até
aqui foram realizados em Windows; jogo, DLL e inspeção sob Proton não foram testados.

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
| DLL | `cdylib` x86, `DllMain` mínimo, worker, UDP, leitura local e estados remotos | Carregar e operar dentro do jogo real |
| Host de teste | `client_probe` carrega a DLL real em processo próprio | Testes de integração com o ciclo do jogo |
| Investigação | `memory_tool` Rust: processos, regiões, busca, filtros, comparação, ponteiros, observação e envio externo | Perfis de versões e ciclo de vida do veículo |
| Coordenadas | Cadeia validada após movimento, reinício e comparação exata com save | Escala física, outros estados/versões e amostragem consistente |
| Multiplayer visual | Nenhuma entidade remota criada no jogo | Descobrir e integrar criação, atualização e remoção |
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
- [ ] Fixar um toolchain testado em `rust-toolchain.toml`, após verificar os
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
- [ ] Transformar a descoberta em perfil nomeado, com hash e diagnóstico de
  versão incompatível; não aplicar offsets por semelhança do nome do arquivo.
- [ ] Resolver a base do módulo no processo alvo e cada cadeia por sessão.
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

- [ ] Implementar launcher/carregador em Rust, com alvo explícito, DLL por
  caminho absoluto e verificações de arquitetura/perfil.
- [ ] Escolher e documentar uma estratégia de carregamento; usar apenas o
  mecanismo necessário. Resolver exports e módulos no processo correto,
  sem transportar endereços locais de funções para outro processo por hipótese.
- [ ] Definir API de configuração com versão/tamanho de estrutura e ABI estável.
  Não depender de alterar variáveis de ambiente de um jogo já iniciado pela Steam.
- [ ] Iniciar após o carregamento e fora de `DllMain`; evitar início duplicado.
- [ ] Expor estado, contadores e último erro de forma consultável. Logs locais
  devem indicar modo real/simulado e perfil selecionado.
- [ ] Encerrar worker e referências ao motor antes de descarregar. Tratar saída
  do jogo, falha de rede, DLL já carregada e inicialização incompleta.
- [ ] Separar falha de leitura temporária de uma versão realmente incompatível.
- [ ] Validar o ciclo em host Rust próprio e depois no processo original, com
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
Não há ainda uma API do motor identificada e validada para essas operações.

- [ ] Investigar criação de veículo/modelo, atualização de transform e remoção,
  usando somente a instalação local e registrando conclusões próprias.
- [ ] Identificar a thread/fase correta para chamadas e a propriedade dos objetos.
- [ ] Confirmar ABI x86: calling convention, `this`, alinhamento, tamanhos,
  registradores e regras de alocação/liberação antes de chamadas `unsafe`.
- [ ] Criar uma fachada Rust pequena para operações do motor; manter offsets e
  chamadas desconhecidas fora do núcleo de rede.
- [ ] Encaminhar comandos por fila limitada até o tick/thread do jogo; não
  manipular entidades a partir do worker UDP.
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
- [ ] Validar ferramentas/servidor nativos em Linux e build cruzado Windows x86
  já no fluxo inicial do segundo PC. O uso do jogo/DLL sob Proton é uma frente
  adicional de compatibilidade, ainda não comprovada; não depende de portar
  o jogo original nem de substituir a DLL por uma biblioteca Linux.

## 6. Ordem de execução recomendada e riscos

O caminho principal é **M0 → M1/M2 → M3/M4 → M5 → teste LAN do MVP**.
M6 amplia gameplay; M7 melhora o pipeline; M8 acompanha cada mudança de risco;
M9 depende de resultados estáveis. Isso expressa dependências técnicas, não uma
instrução para criar agentes paralelos.

Prioridade imediata: integração x86 controlada e perfil de versão, seguida da
descoberta de criação de entidade remota. Investigar cedo esse ponto evita
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

4. Reproduza primeiro o ambiente registrado: `1.98.1-x86_64-pc-windows-gnu`.
   Esses comandos selecionam o toolchain para o diretório do clone:

```powershell
rustup toolchain install 1.98.1-x86_64-pc-windows-gnu
rustup override set 1.98.1-x86_64-pc-windows-gnu
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
| `asset_tools` | Build/execução nativos; fornecer caminho local dos assets | Somente Windows até este registro |
| `multiplayer_server` | Build/execução nativos; pode atender clientes Windows por UDP | Somente Windows até este registro |
| Protocolo e lógica portável do cliente | Build/testes nativos, com testes Win32 excluídos por `cfg` | Somente Windows até este registro |
| `client_dll` no jogo | Gerar DLL PE x86 por compilação cruzada e executá-la em ambiente Win32 | Testada no host de teste Windows; carregamento no jogo pendente |
| `memory_tool` e `client_probe` | Binários Windows via ambiente compatível ou PC Windows | Versões nativas Linux não implementam essas funções |

Após instalar Git, rustup e as ferramentas de compilação nativas da distribuição,
use, num terminal Linux x86_64:

```sh
git clone https://github.com/rodolfoaltenrath/18wos-across-america-mp.git "$HOME/dev/across-america-mp"
cd "$HOME/dev/across-america-mp"
rustup toolchain install 1.98.1
rustup override set 1.98.1
rustup component add rustfmt clippy
cargo build -p asset_tools -p multiplayer_server --locked
cargo test --workspace --all-targets --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p multiplayer_server --locked -- --bind 0.0.0.0:39000
```

Use a mesma ressalva sobre disponibilidade/versão do toolchain descrita no fluxo
Windows. Os comandos Linux acima são instruções a validar no destino, não testes
já executados nesta máquina. A distribuição determina os pacotes de compilador
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
garante suporte a este jogo, ao carregador futuro ou às APIs de inspeção usadas.
Será necessário validar arquitetura, dependências, versão do Proton, prefixo,
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

**Próxima entrega:** perfil versionado e ciclo de carregamento/configuração/
início/parada da DLL x86 dentro do jogo; depois, uma entidade remota visual.

**Antes de começar:** ler os três documentos, conferir Git e toolchain, localizar
a instalação local e conferir hash. Não refazer a busca ampla de memória se a
cadeia documentada funcionar na versão compatível. Não usar PIDs históricos.

**Ao terminar um bloco:** atualizar checkboxes, arquivos alterados, comando/cenário
de validação, resultado, limitações, decisão de arquitetura e próxima dependência.
O remoto e a licença Apache-2.0 já foram definidos pelo proprietário. A publicação
inicial dos fontes foi autorizada sem coautor; releases de binários ainda não
foram solicitadas. Preservar esse contexto ao continuar em outra máquina.
