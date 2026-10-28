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

No Windows, usar `dev_env windows` com GCC/GNU ld da versão ativa, conforme
README. O antigo `rust-lld` x86 falhou na recuperação de panic neste host; não
reutilizar sua configuração. O helper preserva `.cargo/config.toml` local.
A última validação Windows terminou com dispatcher removido, worker parado e
DLL descarregada do jogo. Não reutilizar PID/endereços absolutos do registro.
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
dirigiu, recarregou e fechou o último processo. Prosseguir para setters/modelo e
init/teardown de recursos, antes de inserir/manter um visual entre frames.
A criação e a movimentação de caminhões remotos ainda não foram implementadas.
