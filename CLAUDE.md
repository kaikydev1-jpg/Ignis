# Regras do criador para o Ignis (vale para toda sessão de IA)

- **Sempre em português do Brasil**, com o criador e nos textos do app.
- **Publicar quando estiver pronto:** terminou o trabalho e está 100% funcional (testes locais, fmt, clippy e CI verdes no Windows e no Linux) → suba a versão (`Cargo.toml` e `app/src-tauri/tauri.conf.json`), troque `.github/release/resumo.txt` e as notas em `.github/release/notas.md`, e rode o workflow **Instaladores** com `publicar = sim`. Não pergunte antes. Quem tem o Ignis recebe o aviso e atualiza com um clique.
- **Não publique** o que não passou nos testes ou no CI.
- **Não diga que algo funciona numa placa ou máquina real sem prova nela.** Separe o que foi provado (testes, CI, motor oficial no processador) do que falta provar (TESTE_DO_CRIADOR.md).
- Sem telemetria. Conexões externas só ao GitHub (motores, versões) e ao Hugging Face (modelos).
- Toda retomada começa pelo **ESTADO.md**.
