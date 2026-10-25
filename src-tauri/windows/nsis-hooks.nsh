; 卸载钩子：删除文件之前先把系统代理与代理环境变量还原。
;
; 正常退出时程序自己会还原，但进程被强制结束后 config.json 里会留下 dirty 标记，
; 卸载又会把配置目录一起删掉，注册表里的 127.0.0.1 代理就成了永久残留。
; 这里借 exe 自己的 --restore-proxy 分支读快照还原，无需 NSIS 解析 JSON。

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "正在还原系统代理与代理环境变量…"
  IfFileExists "$INSTDIR\NanoShadowrocket.exe" 0 +2
    ExecWait '"$INSTDIR\NanoShadowrocket.exe" --restore-proxy'
  IfFileExists "$INSTDIR\nano-shadowrocket.exe" 0 +2
    ExecWait '"$INSTDIR\nano-shadowrocket.exe" --restore-proxy'
!macroend
