# Recursos extras do instalador

`vcrt/` recebe, no CI do Windows, as DLLs do Visual C++ (`msvcp140.dll`,
`vcruntime140.dll`, `vcruntime140_1.dll`) tiradas da pasta "Redist" do Visual
Studio do runner. O llama-server oficial do Windows precisa delas e nem todo PC
tem o "Visual C++ Redistributable" instalado. O Ignis copia essas DLLs para a
pasta de cada motor (distribuição local permitida pela licença do Visual Studio).
