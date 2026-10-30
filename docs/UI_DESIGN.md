# Interface do mod e preparação PT-BR

Direção confirmada em 30/10/2026: priorizar telas e simplicidade de entrada;
o proprietário pode testar neste PC, mesmo sem reunir os dois jogadores.
Depois das telas, traduzir integralmente o jogo. Isso altera a prioridade da
próxima rodada: UI local primeiro, LAN somente quando houver mudança concreta.

## Fluxo e linguagem visual

Referência visual confirmada pelo proprietário: alterar **somente o retângulo
do menu esquerdo**. Manter cenário animado e logotipos originais carregados
pelo jogo. Nenhuma imagem de referência/comercial é copiada para os fontes.
Janela principal, preenchimento e hit areas ficam no retângulo nativo
`27 80 347 490` (1024×768, origem inferior), em vez de um overlay fullscreen.
Seis linhas: nova carreira, carregar, Multiplayer, configurações, créditos e
sair. Multiplayer nativo continua indicação inativa; os outros cinco mantêm
IDs/nomes originais. A coluna principal da prévia acompanha esse posicionamento;
o resto da janela é um espaço neutro explicativo, sem reprodução do cenário.
As outras oito telas continuam bases separadas para integração posterior.

Estilo inicial: painel escuro, destaque âmbar, tipografia legível, botões amplos,
tema de estrada. Sem assets comerciais novos, webview ou JavaScript. Layout
lógico 1024×768, escalado com barras laterais quando necessário; os mesmos
retângulos alimentam render e hit testing. Formulários preservam os rascunhos.

| Tela | Base disponível | Integração ainda necessária |
| --- | --- | --- |
| Principal | Painel esquerdo: nova carreira, carregar, Multiplayer, configurações, créditos e sair | Inicialização do mod e callbacks nativos; carregar/créditos são placeholders na prévia |
| Multiplayer | Hospedar/entrar, voltar e exemplos visuais na prévia | Compor início/parada seguros |
| Hospedar | Nome, sala, capacidade, meta exata e sem limite | Escolher save, criar carreira e ligar serviços |
| Entrar | Nome, endereço, procura LAN e validação | Descoberta real, convite/importação/ticket |
| Sala | Revisão, indicação de conexão e saída | Participantes reais e pronto/iniciar |
| Preparação | Estado pendente e cancelamento | Progresso real de hash/transferência/partida |
| Erro | Mensagem traduzida e retorno preservando formulário | Mapear falhas tipadas dos serviços |
| Configurações | Troca de idioma da UI própria | Persistência e demais preferências |
| Pausa | Navegação básica | Identificar pausa/retorno e encerrar sessão sem perder limpeza |

O catálogo próprio está em `mod_ui/src/locale.rs`, com chaves estáveis,
PT-BR/inglês, validação de placeholders e bounds. Trocar o idioma da prévia
não traduz o jogo. Meta financeira usa inteiro em unidades menores; a entrada
brasileira não converte moeda nem altera a condição original de vitória.

`mod_ui` é um quarto crate pequeno, sem sockets, processos, ponteiros ou
dependências do servidor. Evita que o pipeline de assets dependa da DLL, ou
que o servidor precise conhecer telas. Modelo, navegação, catálogo e exportação
ESC podem ser reutilizados pela prévia e pelo futuro adaptador do motor.

## Prévia local e limites

`mp_ui` abre SUA PRÓPRIA janela no Windows, sem procurar/abrir jogo, conectar,
instalar assets ou ler saves. Duplo clique abre a prévia. Tab navega, clique
seleciona e Ctrl+A substitui o conteúdo de um campo; Escape volta/cancela.
Conectar/hospedar emite intenção validada, mas o adaptador de prévia recusa a
execução com mensagem explícita. Exemplos de sala/preparação não são sessões
reais. Sucesso atrasado depois de cancelar não muda a tela nem reconecta.

`--render PASTA-NOVA` usa DIBs fora da tela e gera BMPs próprios. `--self-test`
confere fontes PT-BR da prévia, desenho e recursos GDI. Isso não valida fontes
do jogo, render DirectX, foco, fullscreen ou leitura do motor. No Linux,
`--export PASTA-NOVA` funciona nativamente; a janela Windows sob Proton continua
pendente até haver execução nesse ambiente.

## Formato nativo e menu experimental

Foram encontrados `ui/mmain.esc`, `ui/mnewgame.esc`, `ui/mload.esc`, HUDs e
notebook em SCS; `defs/menu.def` descreve principalmente o fundo 3D.
`ui_tools project` gera arquivos ESC próprios e um projeto de overlay próprio.
As telas `ui/mp/*.esc` NÃO têm handler do motor: são uma base de layout, não
menus de rede já funcionais. Campos nesses exports são representações visuais;
a edição/interação nativas exigem o adaptador. Não chamar construtores/callbacks
desconhecidos por endereço para contornar essa dependência.

Com `--main-menu-preview`, gera também `ui/mmain.esc`: conserva o handler
original e apenas IDs/nomes das cinco ações existentes (10/20/30/40/50).
Multiplayer aparece como indicação inativa "em preparo", sem enviar um ID novo
a um handler que pode não aceitá-lo. A prévia redesenha o menu; não altera os
outros cenários, não implementa a carreira financeira no motor nem faz login.
O pacote agora é 0.2.0; o instalador só aceita a geometria/template desta versão.
Recibos de restauração antigos continuam válidos, sem exigir o template novo.
Semitransparência e aspecto dos componentes dentro do painel ainda precisam
da conferência no motor. Não confundir limites geométricos com essa prova.

`ui_tools install` só aceita esse pacote exato, com dez entradas próprias,
hashes válidos e módulo do perfil suportado. Exige todos os `prism3d.exe`
fechados. Copia/sincroniza backup de `aa/init.cfg` e plano privado antes de
adicionar `aa/rust_mp_ui.scs` e uma montagem p4. Recusa colisão com p4, overlay,
temporário ou destino existente. Prioridade/aparência no motor ainda precisam
da prova local. Não executa o jogo. Restauração fica disponível mesmo após
atualização do módulo; exige config/payload com hashes conhecidos e preserva
alterações do usuário. Backup não é apagado. Arquivos parciais/conflitantes
ficam preservados para diagnóstico; não promete recuperação de disco cheio,
queda de energia ou edição concorrente. Usar numa árvore local sem escritores.

## Próximo adaptador dentro do jogo

1. Validar aparência/ações antigas e restauração do menu experimental neste PC.
2. Identificar registro de telas, eventos e roteamento do handler de menu por
   análise/leitura passiva. Comprovar ABI, thread, ownership e remoção.
3. Ligar o botão Multiplayer e os eventos das telas ao modelo compartilhado;
   filas de trabalho executam hash/rede sem bloquear a UI. A thread do motor
   recebe apenas trabalho permitido, com cancelamento por geração.
4. Reutilizar bootstrap, tickets e encerramento existentes; carregar o mod na
   inicialização fora de DllMain e oferecer um ponto de entrada simples.
5. Validar localmente navegação, fontes, foco, resolução, retorno e desligamento.
   Depois agrupar a prova LAN dos fluxos completos, sem repetir presença básica.

## Tradução integral após as telas

`ui_tools inventory` lê arquivos selecionados dos SCS sem extração, registra
hashes, recursos, referências `@@chave@@`, UTF-8/bytes legados e hints de
codepage. Não exporta a prosa original. Não encontra automaticamente strings
embutidas em módulos, áudio ou imagens; `translation_complete=false` permanece.
Há declarações `codepage:1250` em fontes geradoras e arquivos `.font` que não
são UTF-8. Isso NÃO comprova o codepage ativo nem a cobertura dos glifos do jogo.

As bases Unicode ficam intactas. ESC experimental usa transliteração ASCII
provisória até validar/codificar fontes e textos nativos. A tradução completa
exigirá localizar as tabelas de definição das chaves, inventariar textos literais
e embutidos, criar atlas/fontes com ã/õ/ç/acentos, manter placeholders/PML,
revisar termos de transporte, ajustar layouts e testar HUD/menus/diálogos.
Moeda R$ continua separada: apresentação, valores e balanceamento precisam
de contrato próprio. Não distribuir SCS/fontes/prosa comercial extraídos; manter
o inventário e os backups privados e versionar somente trabalho próprio.

## Evidências locais deste bloco

Rodada do painel esquerdo 0.2.0: verify Windows novamente com 196 x86/243
nativos, releases, Clippy e rustfmt. Nove BMPs x86 e layouts exportados; principal
inspecionado. Pacote gerado, sem instalação ou abertura do jogo. Log privado
`target/analysis/verify-windows-20261030-left-menu.log`; artefatos próprios em
`target/analysis/ui-left-menu-20261030-01/`. A prova de instalação/restauração
real abaixo corresponde ao pacote inicial 0.1.0; G10 cobre o menu 0.2.0.

`dev_env verify` Windows: 196 testes x86 e 243 nativos, builds release,
Clippy nos três escopos e rustfmt passaram. Log privado ignorado em
`target/analysis/verify-windows-20261030-ui-foundation-final.log`.

Nove telas renderizadas em BMP fora da tela; principal e hospedar inspecionados
visualmente. Em 27 desenhos repetidos o processo manteve cinco objetos GDI
antes/depois; os glifos PT-BR da fonte Windows foram encontrados. Não é teste
do menu nativo, fonte do jogo nem interação humana.

Instalação/restauração na árvore real com o jogo fechado passou: status
installed/restored, `aa/init.cfg` com hash idêntico ao backup e overlay ausente.
Nenhum jogo foi aberto. O inventário dos três SCS encontrou 293 chaves únicas
referenciadas em 3.197 recursos selecionados, 19 não UTF-8 e cinco hints de
codepage. Não representa cobertura integral de textos. Artefatos privados em
`target/analysis/ui-foundation-20261030-01/`; não versionar.
