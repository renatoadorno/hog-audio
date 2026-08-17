Crie um Makefile para facilitar o build

Fassa um code review focando em algumas coisas:

Lembra que o Hog Mode sequestra a placa de som? Se o usuário apertar Ctrl+C no terminal e você matar o processo do C++ sem antes avisar o macOS para liberar o Hog Mode, a placa de som do cara vai ficar travada e o Mac inteiro vai ficar mudo até ele reiniciar o serviço de áudio do sistema.

Outra coisa extremamente inportante, o arquivo decodificado pode te entregar amostras de áudio em números inteiros de 16-bit, ou em floats de 32-bit (que é o que o Core Audio prefere internamente). Se você mandar um tipo e o DAC estiver esperando outro, você não vai ouvir música, vai ouvir um ruído branco ensurdecedor capaz de queimar seu fone. Certifique-se de configurar o Core Audio para receber exatamente o tipo de dado que a sua lib de decodificação está cuspindo.

ao final utilize um subagente como o ecc:cpp-reviewer para code review