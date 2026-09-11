#!/bin/sh

set -eu

LOG_FILE="${INSTRUCTION_LOG:-/tmp/mico_aivs_lab/instruction.log}"
TEXT="${*:-}"

if [ -z "$TEXT" ]; then
    echo "用法: $0 <要模拟的最终 ASR 文本>" >&2
    echo "示例: $0 打开次卧台灯" >&2
    exit 2
fi

if [ ! -f "$LOG_FILE" ]; then
    echo "找不到 AIVS 指令日志: $LOG_FILE" >&2
    exit 1
fi

case "$TEXT" in
    *"
"*)
        echo "测试文本不能包含换行符" >&2
        exit 2
        ;;
esac

escape_json() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

ESCAPED_TEXT="$(escape_json "$TEXT")"
DIALOG_ID="ssh-route-$(date +%s)-$$"

printf '%s\n' \
    "{\"header\":{\"dialog_id\":\"$DIALOG_ID\",\"id\":\"ssh-asr-$$\",\"name\":\"RecognizeResult\",\"namespace\":\"SpeechRecognizer\"},\"payload\":{\"is_final\":true,\"is_vad_begin\":true,\"results\":[{\"confidence\":1.0,\"is_nlp_request\":true,\"is_stop\":true,\"text\":\"$ESCAPED_TEXT\"}]}}" \
    >> "$LOG_FILE"

# Music routing needs Dialog.Finish to leave the synthetic wake state. HA routing
# marks the dialog pending and safely defers this event until its request completes.
sleep 1
printf '%s\n' \
    "{\"header\":{\"dialog_id\":\"$DIALOG_ID\",\"id\":\"ssh-finish-$$\",\"name\":\"Finish\",\"namespace\":\"Dialog\"},\"payload\":{}}" \
    >> "$LOG_FILE"

echo "已注入: dialog_id=$DIALOG_ID text=$TEXT"
echo "查看分路: tail -n 80 /tmp/open-xiaoai-client.log"
