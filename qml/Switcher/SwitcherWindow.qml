import QtQuick
import Quickshell
import Quickshell.Wayland
import Nyx.Ui

// The switcher card, centred on the focused screen while a shortcut is held. It takes
// the keyboard (exclusively) so it sees the modifier go up: that switches. Hyprland
// keeps the bound chords (Alt+Tab, …) and they reach the backend as requests, which
// move the selection; other keys come here and go to the backend. The card stays
// transparent while `pending`, so a quick tap switches without anything flashing up.
PanelWindow {
    id: w

    required property var client
    property string pinnedScreen: ""
    readonly property bool keyboardOff: Quickshell.env("NYX_SHELL_NO_KEYBOARD") === "1"
    readonly property var st: client.state.switcher || ({ open: false, items: [], selected: 0 })
    readonly property var thumbs: client.state.thumbs || ({})
    readonly property var items: st.items || []
    readonly property var held: st.held || ({})
    readonly property var target: Quickshell.screens.find(s => s.name === (pinnedScreen || st.screen)) || null
    // Hover selects only once the pointer has moved since the card opened.
    property point pointerAt: Qt.point(-1, -1)
    property bool pointerMoved: false

    visible: st.open === true && target !== null && client.connected
    screen: target
    color: "transparent"
    exclusionMode: ExclusionMode.Ignore
    implicitWidth: card.width + 56
    implicitHeight: card.height + 56
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "nyx-switcher"
    WlrLayershell.keyboardFocus: keyboardOff ? WlrKeyboardFocus.None : WlrKeyboardFocus.Exclusive

    onVisibleChanged: {
        if (visible) {
            pointerAt = Qt.point(-1, -1);
            pointerMoved = false;
            keys.forceActiveFocus();
            client.request("Shown", {}, null);
        } else
            client.request("Closed", {}, null);
    }

    function keyName(k) {
        switch (k) {
        case Qt.Key_Escape:
            return "Escape";
        case Qt.Key_Return:
            return "Return";
        case Qt.Key_Enter:
            return "Enter";
        case Qt.Key_Space:
            return "Space";
        case Qt.Key_Tab:
            return "Tab";
        case Qt.Key_Backtab:
            return "Backtab";
        case Qt.Key_QuoteLeft:
        case Qt.Key_AsciiTilde:
            return "Grave";
        case Qt.Key_Left:
            return "Left";
        case Qt.Key_Right:
            return "Right";
        case Qt.Key_Up:
            return "Up";
        case Qt.Key_Down:
            return "Down";
        case Qt.Key_Home:
            return "Home";
        case Qt.Key_End:
            return "End";
        case Qt.Key_Delete:
            return "Delete";
        case Qt.Key_Backspace:
            return "Backspace";
        }
        return "";
    }

    FocusScope {
        id: keys
        anchors.fill: parent
        focus: true
        Keys.onPressed: e => {
            w.client.request("Key", {
                key: w.keyName(e.key),
                shift: (e.modifiers & Qt.ShiftModifier) !== 0,
                ctrl: (e.modifiers & Qt.ControlModifier) !== 0,
                text: e.text
            }, null);
            e.accepted = true;
        }
        // The shortcut's modifier went up: switch. `modifiers` may still hold the one
        // released, so it's taken out first.
        Keys.onReleased: e => {
            let released = 0;
            if (e.key === Qt.Key_Alt || e.key === Qt.Key_AltGr)
                released = Qt.AltModifier;
            else if (e.key === Qt.Key_Super_L || e.key === Qt.Key_Super_R || e.key === Qt.Key_Meta)
                released = Qt.MetaModifier;
            else if (e.key === Qt.Key_Control)
                released = Qt.ControlModifier;
            if (released === 0 || !(w.held.alt || w.held.super || w.held.ctrl))
                return;
            const left = e.modifiers & ~released;
            const still = (w.held.alt && (left & Qt.AltModifier)) || (w.held.super && (left & Qt.MetaModifier)) || (w.held.ctrl && (left & Qt.ControlModifier));
            if (!still)
                w.client.request("Release", {}, null);
            e.accepted = true;
        }

        Rectangle {
            id: card
            anchors.centerIn: parent
            width: col.implicitWidth + 28
            height: col.implicitHeight + 26
            radius: 15
            color: Style.popover
            border.width: 1
            border.color: Style.border
            opacity: w.st.pending ? 0 : 1
            Behavior on opacity {
                enabled: w.st.animation !== false && !Style.reducedMotion
                NumberAnimation {
                    duration: 90
                    easing.type: Easing.OutCubic
                }
            }

            Column {
                id: col
                x: 14
                y: 14
                spacing: 0

                Label {
                    visible: !!w.st.query
                    text: "“" + (w.st.query || "") + "”"
                    size: 13
                    font.weight: Font.Medium
                    color: Style.accent
                    bottomPadding: 10
                }
                Grid {
                    id: grid
                    anchors.horizontalCenter: parent.horizontalCenter
                    columns: Math.max(1, w.st.columns || 1)
                    spacing: 4
                    Repeater {
                        model: w.items
                        delegate: WindowTile {
                            required property var modelData
                            required property int index
                            item: modelData
                            thumbs: w.thumbs
                            selected: index === w.st.selected
                            apps: w.st.apps === true
                            thumbnails: w.st.thumbnails !== false
                            tile: w.st.tile || ({ tile: 256, w: 232, h: 146, app: 112, window: 184 })
                            onClicked: w.client.request("Activate", { index: index }, null)
                            onPointed: {
                                if (w.pointerMoved && index !== w.st.selected)
                                    w.client.request("Select", { index: index }, null);
                            }
                        }
                    }
                }
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: Math.min(implicitWidth, Math.max(grid.width, 360))
                    horizontalAlignment: Text.AlignHCenter
                    text: w.st.caption || ""
                    size: 14
                    font.weight: Font.DemiBold
                    elide: Text.ElideMiddle
                    topPadding: 10
                }
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: Math.min(implicitWidth, Math.max(grid.width, 360))
                    horizontalAlignment: Text.AlignHCenter
                    text: w.st.detail || ""
                    size: 12
                    color: Style.dimText
                    elide: Text.ElideRight
                    topPadding: 2
                }
            }
        }

        HoverHandler {
            onPointChanged: {
                const p = point.position;
                if (w.pointerAt.x < 0)
                    w.pointerAt = p;
                else if (Math.abs(p.x - w.pointerAt.x) + Math.abs(p.y - w.pointerAt.y) > 3)
                    w.pointerMoved = true;
            }
        }
    }
}
