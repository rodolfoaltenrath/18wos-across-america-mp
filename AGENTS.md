# Orientações para continuar o projeto

Leia `ROADMAP.MD` e `README.md` antes de implementar. O roadmap registra o estado
validado, as dependências, os critérios de conclusão e a próxima etapa.

O próximo computador do proprietário usa **Linux**. Preparar o desenvolvimento
nativo de assets/servidor e a compilação cruzada da DLL Windows x86. O projeto
ainda não foi testado em Linux/Proton. Não tentar executar o toolchain host
Windows nativamente no Linux nem tratar build cruzado como teste do jogo.

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
  explicitamente o commit/envio inicial do projeto, sem coautor. Preservar o
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

Implementar a integração da DLL x86 com o jogo: perfil de versão, configuração
explícita, carregamento, início/parada controlados e diagnóstico. Validar o ciclo
primeiro em host Rust próprio; depois usar a instalação original. A criação e a
movimentação de caminhões remotos ainda não foram implementadas.
