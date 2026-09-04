import QtQuick
import Omareel.Ui

OmIconButton {
    id: control
    property color toolIconColor: enabled ? theme.textMuted : theme.textFaint
    icon.color: toolIconColor
}
