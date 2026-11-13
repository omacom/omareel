import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts
import Omareel.Ui

// Editing with the person's coding agent (the one chosen in Omarchy): Polish, "Ask the
// editor…" and "Write description". Nothing runs until the person starts it: the first time,
// the drawer says which agent runs and what goes to its provider, with a Start button; after
// that (remembered per agent, until "Ask first"), the actions and Ctrl+Shift+P run it directly.
// The agent edits through `omareel apply`, so each change shows in the editor as it lands, as
// one undo step.
Rectangle {
    id: root
    color: theme.surface
    signal closeRequested()
    readonly property bool running: agent.running
    readonly property var chapterData: agent.chapters
    readonly property var markers: agent.chapters.exists ? agent.chapterMarkers(JSON.stringify(editor.clips)) : []
    property int applies: 0
    property string copied: ""
    // A run asked for before the person allowed this agent: the Start button runs it.
    property string pendingTask: ""
    property string pendingRequest: ""

    // Runs `task` now if the person allowed this agent before; else waits for Start.
    function request(task, text) {
        if (agent.consented) start(task, text)
        else {
            pendingTask = task
            pendingRequest = text || ""
        }
    }
    function consentAndStart() {
        agent.grantConsent()
        if (pendingTask) start(pendingTask, pendingRequest)
    }
    function start(task, request) {
        pendingTask = ""
        pendingRequest = ""
        log.clear()
        root.applies = 0
        // The agent works from project.json: what was edited here goes there first.
        const saved = typeof window !== "undefined" && window.saveForAgent ? window.saveForAgent() : editor.saveNow()
        if (!saved) {
            log.append({ kind: "problem", text: "The project couldn't be saved, so the agent didn't start.", path: "" })
            return
        }
        if (task === "polish") agent.polish()
        else if (task === "chapters") agent.writeDescription()
        else agent.ask(request)
    }
    function startText() {
        if (pendingTask === "polish") return "Start polishing"
        if (pendingTask === "chapters") return "Start writing the description"
        if (pendingTask === "ask") return "Start"
        return "Allow " + agent.agentName
    }
    function focusPrompt() { if (prompt.enabled) prompt.forceActiveFocus() }
    function timeText(seconds) {
        const total = Math.max(0, Math.floor(seconds))
        const minutes = Math.floor(total / 60)
        const rest = total % 60
        return (minutes < 10 ? "0" : "") + minutes + ":" + (rest < 10 ? "0" : "") + rest
    }
    function runningText() {
        if (agent.task === "polish") return "Polishing…"
        if (agent.task === "chapters") return "Writing the description…"
        return "Working on it…"
    }

    // The side panel underneath gets none of the drawer's clicks, wheels or hovers.
    MouseArea { anchors.fill: parent; hoverEnabled: true; acceptedButtons: Qt.AllButtons; onWheel: wheel => wheel.accepted = true }
    Rectangle { anchors.left: parent.left; width: 1; height: parent.height; color: theme.hairline }

    ListModel { id: log }

    Connections {
        target: agent
        function onEntry(kind, text, detail) {
            if (kind === "apply" && detail.ok) root.applies += 1
            if (kind === "finished") {
                // The agent's last words usually came as its last message already.
                const last = log.count ? log.get(log.count - 1) : null
                const stats = []
                if (detail.duration_ms) stats.push((detail.duration_ms / 1000).toFixed(1) + " s")
                if (detail.cost_usd !== undefined) stats.push("$" + detail.cost_usd.toFixed(detail.cost_usd < 0.1 ? 4 : 2))
                else if (detail.tokens) stats.push(detail.tokens + " tokens")
                const done = (detail.ok ? "Done" : "Ended with an error") + (stats.length ? " · " + stats.join(" · ") : "")
                text = last && last.kind === "message" && last.text === text ? done : text + "\n" + done
            }
            log.append({ kind: kind, text: text, path: detail.path || "" })
            entries.follow = true
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.leftMargin: 18
        anchors.rightMargin: 14
        anchors.topMargin: 14
        anchors.bottomMargin: 14
        spacing: 12

        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            IconImage {
                Layout.preferredWidth: 16; Layout.preferredHeight: 16
                source: "qrc:/omareel/assets/icons/lucide/sparkles.svg"
                sourceSize: Qt.size(16, 16)
                color: theme.accent
            }
            Label {
                Layout.fillWidth: true
                textFormat: Text.PlainText
                text: agent.available ? "Edit with " + agent.agentName + (agent.experimental ? " (experimental)" : "") : "Edit with a coding agent"
                font.pixelSize: theme.type.title
                font.weight: Font.DemiBold
                color: theme.textStrong
                elide: Text.ElideRight
            }
            OmIconButton {
                icon.source: "qrc:/omareel/assets/icons/lucide/x.svg"
                Accessible.name: "Close"
                tooltipText: "Close"
                onClicked: root.closeRequested()
            }
        }

        // Why it can't run.
        Text {
            visible: !agent.available
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            textFormat: Text.PlainText
            font.family: theme.fontFamily
            font.pixelSize: theme.type.small
            color: theme.record
            text: agent.problem
        }

        // What runs, and what leaves this computer, before anything does: the first time, with
        // the Start button that lets it run.
        Rectangle {
            objectName: "agentConsent"
            visible: agent.available && !agent.consented
            Layout.fillWidth: true
            implicitHeight: consentColumn.implicitHeight + 24
            radius: theme.controlRadius
            color: theme.raised
            border.width: 1
            border.color: theme.strokeStrong
            ColumnLayout {
                id: consentColumn
                anchors.fill: parent
                anchors.margins: 12
                spacing: 10
                Text {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    textFormat: Text.PlainText
                    font.family: theme.fontFamily
                    font.pixelSize: theme.type.small
                    color: theme.foreground
                    text: agent.agentName + " runs only when you start it. It reads this recording's timeline, "
                        + "its transcript and frames of the video, and sends them to "
                        + (agent.provider || "its provider") + ". It changes only this recording, through omareel apply."
                }
                Text {
                    visible: agent.experimental
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    textFormat: Text.PlainText
                    font.family: theme.fontFamily
                    font.pixelSize: theme.type.small
                    color: theme.record
                    text: "Codex support is experimental: the commands it runs can read other files on this computer, "
                        + "and what they read goes to " + (agent.provider || "its provider") + " too."
                }
                Text {
                    visible: root.pendingTask === "ask" && root.pendingRequest !== ""
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    textFormat: Text.PlainText
                    font.family: theme.fontFamily
                    font.pixelSize: theme.type.small
                    color: theme.textMuted
                    text: "“" + root.pendingRequest + "”"
                }
                Text {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    textFormat: Text.PlainText
                    font.family: theme.fontFamily
                    font.pixelSize: theme.type.small
                    color: theme.textFaint
                    text: "After this, Polish and Ctrl+Shift+P start " + agent.agentName + " right away, until you choose Ask first."
                }
                OmButton {
                    objectName: "agentConsentStart"
                    Layout.fillWidth: true
                    primary: true
                    enabled: !root.running
                    text: root.startText()
                    icon.source: "qrc:/omareel/assets/icons/lucide/sparkles.svg"
                    onClicked: root.consentAndStart()
                }
            }
        }

        // Once allowed: what it reads and sends, what can be undone, and taking it back.
        Text {
            visible: agent.available && agent.consented
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            textFormat: Text.PlainText
            font.family: theme.fontFamily
            font.pixelSize: theme.type.small
            color: theme.textMuted
            text: agent.agentName + " reads this recording's timeline, transcript and frames"
                + (agent.provider ? ", which go to " + agent.provider : "")
                + ", and edits it with omareel apply. Each edit is one undo step; chapters and the "
                + "description are saved on their own and can't be undone."
                + (agent.experimental ? " Codex support is experimental: its commands can read other files on this computer." : "")
        }
        OmButton {
            objectName: "agentRevoke"
            visible: agent.available && agent.consented && !root.running
            text: "Ask first"
            tooltipText: "Polish and Ctrl+Shift+P will show this drawer again before " + agent.agentName + " runs"
            onClicked: agent.revokeConsent()
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            OmButton {
                Layout.fillWidth: true
                // The Start button above is the one call to action until the agent is allowed.
                primary: agent.consented
                bordered: !agent.consented
                enabled: agent.available && !root.running
                text: "Polish"
                icon.source: "qrc:/omareel/assets/icons/lucide/sparkles.svg"
                tooltipText: "Trim, cut idle stretches and pauses, fix zooms"
                tooltipShortcut: "Ctrl Shift P"
                onClicked: root.request("polish", "")
            }
            OmButton {
                Layout.fillWidth: true
                bordered: true
                enabled: agent.available && !root.running
                text: "Write description"
                icon.source: "qrc:/omareel/assets/icons/lucide/bookmark.svg"
                tooltipText: "Chapters, a title and a description, for YouTube"
                onClicked: root.request("chapters", "")
            }
        }

        OmTextField {
            id: prompt
            objectName: "agentPrompt"
            Layout.fillWidth: true
            enabled: agent.available && !root.running
            placeholderText: "Ask the editor…"
            selectByMouse: true
            onAccepted: {
                if (text.trim().length === 0) return
                root.request("ask", text)
                text = ""
            }
            Keys.onEscapePressed: event => { focus = false; window.restoreEditorFocus(); event.accepted = true }
        }
        Text {
            visible: agent.available && log.count === 0 && !root.running
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            textFormat: Text.PlainText
            font.family: theme.fontFamily
            font.pixelSize: theme.type.small
            color: theme.textFaint
            text: "For example: “cut everything before I open the terminal”, “make it under 60 seconds”, “zoom on the error message”."
        }

        RowLayout {
            visible: root.running
            Layout.fillWidth: true
            spacing: 8
            BusyIndicator { running: root.running; Layout.preferredWidth: 18; Layout.preferredHeight: 18 }
            Label {
                Layout.fillWidth: true
                textFormat: Text.PlainText
                text: root.runningText() + (root.applies ? "  ·  " + root.applies + (root.applies === 1 ? " change" : " changes") : "")
                color: theme.textMuted
                font.pixelSize: theme.type.small
                elide: Text.ElideRight
            }
            OmButton {
                bordered: true
                destructive: true
                text: "Stop"
                icon.source: "qrc:/omareel/assets/icons/lucide/square.svg"
                onClicked: agent.stop()
            }
        }

        OmSeparator { Layout.fillWidth: true; visible: log.count > 0 }

        ListView {
            id: entries
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: 8
            model: log
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
            // Stays at the newest entry, also as images load, until scrolled away from it.
            property bool follow: true
            onMovementEnded: follow = atYEnd
            onContentHeightChanged: if (follow) Qt.callLater(positionViewAtEnd)
            delegate: Item {
                id: entry
                required property string kind
                required property string text
                required property string path
                width: ListView.view.width - 8
                implicitHeight: row.implicitHeight
                readonly property bool isApply: kind === "apply"
                readonly property bool isProblem: kind === "problem" || kind === "stopped" || (isApply && text.startsWith("Apply failed"))
                RowLayout {
                    id: row
                    width: parent.width
                    spacing: 8
                    IconImage {
                        visible: entry.isApply || entry.kind === "finished"
                        Layout.alignment: Qt.AlignTop
                        Layout.topMargin: 2
                        Layout.preferredWidth: 14; Layout.preferredHeight: 14
                        source: "qrc:/omareel/assets/icons/lucide/check.svg"
                        sourceSize: Qt.size(14, 14)
                        color: entry.isProblem ? theme.record : theme.accent
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 6
                        Text {
                            Layout.fillWidth: true
                            visible: entry.kind !== "image"
                            wrapMode: Text.Wrap
                            textFormat: Text.PlainText
                            maximumLineCount: entry.kind === "command" ? 2 : 40
                            elide: Text.ElideRight
                            font.family: entry.kind === "command" ? theme.monoFamily : theme.fontFamily
                            font.pixelSize: entry.kind === "message" || entry.kind === "finished" ? theme.type.body : theme.type.small
                            font.weight: entry.isApply ? Font.DemiBold : Font.Normal
                            color: entry.isProblem ? theme.record
                                : entry.kind === "command" || entry.kind === "started" ? theme.textFaint
                                : entry.isApply ? theme.textStrong : theme.foreground
                            text: entry.kind === "command" ? "$ " + entry.text : entry.text
                        }
                        Image {
                            visible: entry.kind === "image"
                            Layout.fillWidth: true
                            Layout.preferredHeight: visible ? Math.min(180, width * (implicitHeight / Math.max(1, implicitWidth))) : 0
                            fillMode: Image.PreserveAspectFit
                            horizontalAlignment: Image.AlignLeft
                            asynchronous: true
                            cache: false
                            source: entry.kind === "image" && entry.path ? "file://" + entry.path : ""
                        }
                    }
                }
            }
        }

        // chapters.json, on the edited timeline.
        ColumnLayout {
            visible: !!root.chapterData.exists
            Layout.fillWidth: true
            spacing: 6
            OmSeparator { Layout.fillWidth: true }
            Label {
                Layout.fillWidth: true
                textFormat: Text.PlainText
                text: root.chapterData.title || "Chapters"
                font.weight: Font.DemiBold
                color: theme.textStrong
                wrapMode: Text.Wrap
            }
            Text {
                Layout.fillWidth: true
                visible: !!root.chapterData.description
                text: root.chapterData.description || ""
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                color: theme.textMuted
                font.pixelSize: theme.type.small
            }
            Repeater {
                model: root.markers
                delegate: RowLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: 8
                    Label {
                        textFormat: Text.PlainText
                        text: root.timeText(modelData.time)
                        font.family: theme.monoFamily
                        font.pixelSize: theme.type.small
                        color: theme.accent
                        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: editor.seek(modelData.time) }
                    }
                    Label {
                        Layout.fillWidth: true
                        textFormat: Text.PlainText
                        text: modelData.title
                        elide: Text.ElideRight
                        font.pixelSize: theme.type.small
                        color: theme.foreground
                    }
                }
            }
            RowLayout {
                Layout.fillWidth: true
                OmButton {
                    bordered: true
                    text: root.copied ? "Copied" : "Copy YouTube description"
                    icon.source: "qrc:/omareel/assets/icons/lucide/copy.svg"
                    onClicked: {
                        clipboardText.text = agent.youtubeDescription(JSON.stringify(editor.clips), editor.duration)
                        clipboardText.selectAll()
                        clipboardText.copy()
                        root.copied = "yes"
                        copiedTimer.restart()
                    }
                }
                Item { Layout.fillWidth: true }
            }
            Timer { id: copiedTimer; interval: 2000; onTriggered: root.copied = "" }
            // The clipboard, from QML.
            TextEdit { id: clipboardText; visible: false; textFormat: TextEdit.PlainText }
        }
    }
}
