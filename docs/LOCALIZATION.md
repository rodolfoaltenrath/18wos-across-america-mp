# Tradução PT-BR — textos e dublagem

Decisão do proprietário em 30/10/2026: traduzir o jogo inteiro, **textos e
dublagem em etapas separadas**. A primeira versão traduzida deve poder ser
testada sem esperar pela produção das vozes. Moeda R$ continua requisito
separado (M7): os valores em `$` foram mantidos na tradução.

## Etapa T1 — tabela `scripts/local.def` (entregue, sem teste no jogo)

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

## Etapa T2 — texto fora da tabela (pendente)

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

Próximo passo da T2: um extrator por tipo de arquivo, com catálogo próprio por
origem (mesmo esquema de hash) e reconstrução só dos arquivos tocados. A
codificação dos `.esc` precisa de decisão comprovada no jogo (G11) antes.

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
