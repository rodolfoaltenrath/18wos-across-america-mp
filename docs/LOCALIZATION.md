# Tradução PT-BR — textos e dublagem

Decisão do proprietário em 30/10/2026: traduzir o jogo inteiro, **textos e
dublagem em etapas separadas**. A primeira versão traduzida deve poder ser
testada sem esperar pela produção das vozes. Moeda R$ continua requisito
separado (M7): os valores em `$` foram mantidos na tradução.

## Etapa T1 — tabela `scripts/local.def` (entregue; retorno inicial positivo)

Em 30/10 o proprietário informou que testou a tradução e, até onde verificou,
ficou boa. Pediu trocar a fonte por achá-la quadrada. É um relato humano geral:
não especifica telas, resolução, cobertura de glifos ou restauração; esses itens
de G11 continuam pendentes. Preservar o teste já feito e concentrar a próxima
conferência nas fontes novas e em eventuais cortes.

O jogo procura textos por chave (`@@chave@@`) em `scripts/local.def`. O arquivo
visível é o do `aa/patch_1_10.scs` (prioridade p3), cifrado com o envelope 3nK;
o do `base.scs` é texto puro, o que mostra que o motor aceita a versão sem cifra.
São 496 chaves únicas, 12 delas de configuração de fonte (`__fnt_*`, nunca
traduzidas) e **484 traduzíveis**. O original tem chaves duplicadas
(`qh_to_the_city`, `ui_gfx_device`) e `ui_game_over` sem aspas de abertura;
a reconstrução aplica a tradução em todas as ocorrências e corrige essa linha.

- Catálogo próprio: `translations/pt-BR/local_def.json` (schema 1). Guarda só o
  texto PT-BR por chave e `src`, os 12 primeiros hex do SHA-256 do valor inglês,
  para detectar mudança na origem. O texto inglês nunca entra no Git.
- Cobertura atual: **484/484** chaves, sem erros de marcação nem codificação.
  Códigos de cor `\\y`/`\\w`/`\\g` e `<br>` são conferidos em ordem.
- Termos fixados: frete (job), carga (cargo), reboque (trailer), sede
  (headquarters), oficina (service station), concessionária (truck dealer),
  balança (weigh station), Caderno (Notebook), rádio PX (CB). Nomes de
  caminhões e "Jukebox" ficam como no original.
- Avisos de comprimento restantes: `qh_btn_sell_part` e `qh_loan_10000`, ambos
  dicas de mouse. Layout real só se confirma no jogo (G11).

### Codificação e fontes

O motor indexa as fontes por **byte** (um glifo por valor 0–255). As 15 fontes
do jogo trazem os acentos tchecos da página 1250 e só 6 dos caracteres PT-BR
(`Á Ú á é í ó`). O texto traduzido é gravado em um byte por caractere
(Windows-1252, igual a Latin-1 nesse intervalo). O conjunto permitido é ASCII
imprimível mais `ÀÁÂÃÇÉÊÍÓÔÕÚÜàáâãçéêíóôõúü`. Ficam proibidos aspas, `ª`, `º`,
travessões e reticências tipográficas.

`l10n_tools build` sintetiza os 21 glifos faltantes em 13 fontes:
- A letra-base e o acento agudo são copiados da própria fonte. O agudo vem de
  `á`/`Á` comparados com `a`/`A`.
- O grave é o agudo espelhado. Nas fontes em que o agudo tem 1 px de largura,
  usa-se um desenho diagonal.
- Circunflexo, til, trema e cedilha são padrões de pixels dimensionados pela
  altura do agudo.
- O atlas TGA dobra de altura e os glifos novos ficam abaixo dos pixels
  originais, que não mudam.
- Os `.shd` das variantes (normal, `_3d`, `_s`) recebem a nova altura de
  referência.
- As linhas novas são anexadas ao `.font`, mantendo o restante byte a byte.

`fonts/default.font` (`data.scs`, só ASCII, shader sem `UI()`) fica de fora e
é relatado em `skipped_fonts`. A prévia PNG de cada fonte foi inspecionada
visualmente fora do jogo: legível, alinhada, til distinto de acento duplo.
**Isso não prova a renderização no motor.**

### Comandos

```
l10n_tools check --catalog translations/pt-BR/local_def.json
l10n_tools build --catalog translations/pt-BR/local_def.json --output target/analysis/NOVO/project
asset_tools mod-build target/analysis/NOVO/project/project.json target/analysis/NOVO/package
asset_tools mod-verify target/analysis/NOVO/package
# Jogo fechado; backup novo; monta em p5, acima do menu experimental (p4):
ui_tools install --kind ptbr-text --game . --package target/analysis/NOVO/package --backup target/analysis/NOVO/backup
ui_tools status  --game . --backup target/analysis/NOVO/backup
ui_tools restore --game . --backup target/analysis/NOVO/backup
```

- `check` e `build` usam por padrão os três SCS na ordem de `aa/init.cfg`.
- `stamp` copia um catálogo para um arquivo novo, preenchendo `src` vazios.
- `read` mostra uma entrada decifrada; a saída é privada.

O projeto gerado é inteiro `derived_local`: mistura a tradução com a estrutura e
os pixels lidos da instalação local. Por isso o pacote nunca é distribuível
(`mod-build --distributable` recusa). Cada jogador reconstrói o pacote a partir
dos próprios arquivos. O empacotador agora aceita `.font` e texto de um byte por
caractere em `.def`/`.font`, ainda recusando caracteres de controle; `.esc`/
`.shd`/`.mat`/`.txt` continuam exigindo UTF-8.

A montagem é generalizada em `ui_tools`: o backup registra o tipo do pacote, e
`status`/`restore` o descobrem sozinhos. Com menu e tradução montados,
restaurar na ordem inversa da instalação; a ordem errada é recusada sem alterar
nada (teste próprio).

Primeira geração real, com o catálogo completo:
- Projeto com 56 arquivos e `overlay.scs` de 2.315.676 bytes, aprovado por
  `mod-verify`.
- Artefatos privados em `target/analysis/l10n-ptbr-20261030-01/` (ignorado).
- Não houve instalação nem abertura do jogo.

### Tipografia suavizada — alternativa TrueType, 30/10

`l10n_tools build --typeface ARQUIVO.ttf` agora substitui os contornos de ASCII
imprimível e dos acentos PT-BR por glifos reais da fonte fornecida. A leitura e
rasterização usam Rust (`fontdue`), sem GDI ou dependência de fontes do Windows.
O arquivo é local, não acompanha o projeto/pacote, e deve conter todos os glifos
exigidos. Hash SHA-256 e nome da face ficam no relatório, sem o caminho privado.

Preparada nesta máquina a alternativa **Carlito Bold**: curvas mais suaves e
traço mais legível nos tamanhos pequenos que a Regular. Não é uma mudança da
fonte da janela de prévia (`mp_ui` já usa Segoe UI); altera os atlas do jogo
gerados pelo pipeline de tradução. A suavização continua limitada pela resolução
bitmap dos textos originais; não implementa vetores em tempo real nem aumenta
as dimensões da UI.

- Conserva avanço das letras, kerning, `vert_span`, `line_spacing` e referências
  aos shaders. Espaços, números e largura de medição continuam na grade antiga.
- Os contornos partilham uma escala vertical que cabe na linha, incluindo
  acentos/descendentes. Só letras mais largas são reduzidas horizontalmente por
  média de cobertura; não se alargam letras estreitas artificialmente.
- Alpha suavizado com reforço de cobertura inteiro, determinístico, sem limiar
  binário. `--font-strength 0..100` controla o contraste; padrão 40. Não muda o
  peso da face instalada nem procura outra fonte silenciosamente.
- Os pixels originais e os slots não latinos/especiais ficam preservados. As
  substituições ocupam uma área nova do atlas, e só seus registros são refeitos.
- `default.font`, sem shader UI, continua fora. Falha de outra fonte UI interrompe
  o build antes de criar o projeto, em vez de deixar uma fonte antiga silenciosa.
- Prévia agora usa avanço e kerning reais; foi removido o pixel extra artificial
  entre todas as letras. `*-comparison.png`: fonte anterior em cima, nova embaixo.

Exemplo a partir da raiz do jogo (ajustar caminhos para os seus arquivos):

```text
l10n_tools build --catalog REPO/translations/pt-BR/local_def.json --typeface CAMINHO/Carlito-Bold.ttf --font-strength 40 --output PASTA-EXISTENTE/projeto-NOVO
asset_tools mod-build PASTA-EXISTENTE/projeto-NOVO/project.json PASTA-EXISTENTE/pacote-NOVO
asset_tools mod-verify PASTA-EXISTENTE/pacote-NOVO
```

Sem `--typeface`, o modo anterior de acrescentar acentos continua disponível.
O projeto tipográfico é `ptbr-text` 0.2.0, ainda **derived_local**, com o mesmo
tipo de montagem p5 e fluxo de restauração. Restaurar primeiro a tradução
anterior usando seu backup, depois instalar a nova com outro backup; não copiar
um SCS por cima de uma montagem existente. Menu experimental p4 deve ficar
intacto nessa troca. Não há troca automática de arquivos nem abertura do jogo.

Projeto, pacote e prévias desta preparação ficam em
`target/analysis/l10n-carlito-bold-20261030/`. Os arquivos são privados/ignorados;
nenhuma fonte, atlas, prosa inglesa ou pacote comercial derivado vai ao Git.
Nova aparência no motor ainda pendente de G11.

Validação desta alternativa: 196 testes x86 sob Proton/253 nativos, releases,
Clippy dos escopos padrão e rustfmt; build/Clippy x86 adicionais de asset_tools.
O executável Windows gerou exatamente os mesmos arquivos que o nativo (56
entradas, 26 PNGs e metadados). Pacote 0.2.0 com 3.680.397 bytes aprovado por
`mod-verify`, conferido novamente após interrupção por energia. O primeiro
verify interrompido foi preservado e repetido integralmente com sucesso.
Instruções locais em `target/analysis/l10n-carlito-bold-20261030/APLICAR-NESTE-PC.txt`;
nenhuma instalação/abertura automática. Esses números são bancada/pipeline,
não provas de acentos/filtragem/layout na partida.

## Etapa T2 — texto fora da tabela (primeiro bloco entregue offline)

`localization::literals` inventaria campos sem exportar a prosa original por
padrão. O catálogo próprio `translations/pt-BR/literal_def.json` traz **208
traduções em 207 arquivos**: 148 nomes de componentes de reboques, 58 nomes de
pinturas e os dois textos da carreira milionária. Conserva modelos/marcas de
caminhões e motores, cidades, empresas, motoristas, `Zebra` (já igual em PT-BR)
e o nome geográfico de uma pintura. Os outros cenários originais não receberam
traduções neste catálogo; não foram acrescentados ao fluxo do mod.

Cada arquivo tem SHA-256 completo da origem decodificada; cada ocorrência tem
chave `atributo#ordinal` e hash do valor. A reconstrução substitui somente os
bytes entre aspas dos campos permitidos. Preços, potência, peças/modelos,
regras financeiras, comentários, referências `@@`, PML e todos os demais bytes
ficam intactos. Mudança na origem, caractere sem glifo, aspas/controle/escape,
marcação nova ou placeholders diferentes impedem a geração. O texto da meta
continua em `$`, com o valor original: não implementa R$, rebalanceamento,
`Sem limite` nem altera a condição de vitória do motor.

O inventário desta instalação encontrou 218 campos elegíveis, 1.729 protegidos
ou adiados e **36 recursos com sintaxe incompleta** (aspas de strings sem
fechamento nos originais). Esses recursos são registrados em
`unreadable_resources` e permanecem intocados; nenhuma correção automática do
jogo é feita. Editor (`seg_server`, `prefabs`) fica fora do levantamento.
O inventário é parcial por política e não comprova toda a tradução T2.

Novos comandos, a partir da raiz do jogo; ajustar `REPO` e saídas:

```text
l10n_tools literal-scan --output PASTA-EXISTENTE/inventario-NOVO.json
l10n_tools literal-check --catalog REPO/translations/pt-BR/literal_def.json
l10n_tools build --catalog REPO/translations/pt-BR/local_def.json --literals REPO/translations/pt-BR/literal_def.json --typeface CAMINHO/Carlito-Bold.ttf --output PASTA-EXISTENTE/projeto-NOVO
asset_tools mod-build PASTA-EXISTENTE/projeto-NOVO/project.json PASTA-EXISTENTE/pacote-NOVO
asset_tools mod-verify PASTA-EXISTENTE/pacote-NOVO
```

`--archive` repetido escolhe SCS por prioridade crescente, como em T1.
`literal-scan --include-source` exporta a prosa original **somente para autoria
privada**, nunca Git. `literal-compose --choices ESCOLHAS.json --output NOVO.json`
associa um mapa `SHA256-do-valor → texto PT-BR` aos campos editáveis locais;
recusa hashes ausentes e não copia prosa inglesa. Saídas dos dois comandos
exigem arquivo novo e pasta existente. Catálogos não alteram os SCS originais.

Com `--literals`, o projeto é `ptbr-text` **0.3.0**, ainda derived_local. O
relatório separa `literal_def` (contagem/erros) da tabela; `all_t2_translated` e
`game_tested` continuam false. Sem a opção, os builds T1/fontes antigos seguem
disponíveis. Pacote combinado preparado em
`target/analysis/l10n-t2-20261030/`, sem montagem ou abertura do jogo.

Bancada deste bloco: 196 x86 sob Proton/262 nativos no verify completo,
releases, Clippy/fmt; mais nove testes específicos do tradutor/instalador x86
sob Proton e build/Clippy x86 da ferramenta. Catálogo e projeto gerados pelo
Windows x86 são idênticos ao nativo, inclusive após a revisão final das
recusas. Pacote 0.3.0 conferido, 263 entradas/3.776.468 bytes. Instruções locais
em `APLICAR-NESTE-PC.txt` junto ao pacote. Essas provas não substituem G11.

O instalador `ui_tools install --kind ptbr-text` aceita essas definições somente
após comparar seu conteúdo com os arquivos visíveis dos três SCS locais. Uma
alteração fora dos campos de texto autorizados é recusada **antes do backup ou
de qualquer escrita na instalação**. Para esse bloco T2, exige as três montagens
originais p1/p2/p3 e aceita também o menu próprio p4; outra montagem/configuração
exige investigação em vez de adivinhar a precedência. Fixtures verificam
instalação/restauração e rejeição de mudanças em regras/peças/comentários.
Não confundir essas fixtures com instalação real neste jogo.

Restaurar a tradução anterior pelo backup antes de instalar 0.3.0 com backup
novo. Manter o menu experimental p4. Agrupar a prova dos novos rótulos com
tipografia/layout em G11; não precisa repetir LAN nem usar outro computador.

### Restante de T2

Levantamento por contagem, sem copiar prosa, de atributos literais com frases
em inglês fora de `local.def`. Fora do editor (`seg_server`, `prefabs`), os
principais grupos são:

| Origem | Conteúdo | Observação |
|---|---|---|
| `makes/**/!desc.def`, `description.def` | ~389 nomes/descrições de peças e caminhões | Muitos são nomes próprios; separar descrição de marca |
| `defs/**` `text`/`name`/`instructions` | ~130: empresas, cidades, cargas, cenários, motoristas | Nomes de cidade e empresa devem ficar no original |
| `ui/**.esc` `text`/`value`/`name` | ~130 literais de telas | ESC exige UTF-8 no empacotador; decidir codificação antes |
| atributos `*_pml` (p/n/s/s2/d) | ~150 trechos de texto formatado | Investigar a sintaxe PML antes de traduzir |
| `help.html`, `readme.txt`, `help/*.png` | ajuda fora dos SCS, texto em imagens | Fora do pacote de overlay; tratar à parte |

Ainda pendentes: PML de descrições, literais ESC, ajuda externa/imagens e os
36 arquivos malformados. A codificação dos `.esc` precisa de decisão comprovada
no jogo (G11); o extrator os classifica como adiados, sem aplicar substituições.

## Etapa T3 — dublagem (planejada, etapa própria)

Inventário por contagem: `base.scs` tem 409 OGG. São 222 em `sounds/voices`,
falas de rádio/PX ligadas a `defs/voices/*.def`, e o restante é efeito, motor
e música (sem fala). O plano, sem produção de áudio neste bloco:

1. Mapear cada `defs/voices/*.def` → OGG e o contexto (pedido, resposta,
   comentário), gerando um roteiro próprio só com chaves e texto PT-BR.
2. Definir elenco/direção e formato (taxa, canais, volume) igual ao original,
   medido sem extrair falas para o Git.
3. Gravar ou sintetizar com licença compatível com redistribuição; o áudio
   novo pode ser `owned` (distribuível), ao contrário das fontes derivadas.
4. Overlay próprio de áudio com o mesmo fluxo de montagem e restauração.
5. Validar no jogo: sincronia com legenda/comando, volume e ausência de falas
   faltantes.

Nenhum desses passos foi iniciado; não marcar dublagem como em andamento.
