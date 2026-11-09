import QtQuick
import Quickshell
import Quickshell.Wayland
import Nyx.Ui

// The overview (Super+Alt+W, or the dock's "Show windows"): every window on every
// workspace and monitor, grouped by workspace, over the whole focused screen. It stays
// open without a key held: type to search, Enter or a click switches, Esc clears the
// search, then closes. Each window offers its titlebar, ungroup, float/tile, move and
// close, by button and by key.
PanelWindow {
    id: w

    required property var client
    property string pinnedScreen: ""
    readonly property bool keyboardOff: Quickshell.env("NYX_SHELL_NO_KEYBOARD") === "1"
    readonly property var ov: client.state.overview || ({ open: false })
    readonly property var thumbs: client.state.thumbs || ({})
    readonly property var groups: ov.groups || []
    readonly property var flat: groups.reduce((a, g) => a.concat(g.items), [])
    readonly property var target: Quickshell.screens.find(s => s.name === (pinnedScreen || ov.screen)) || null
    property string selectedAddress: ""
    readonly property int selectedIndex: Math.max(0, flat.findIndex(i => i.address === selectedAddress))
    readonly property var selectedItem: flat[selectedIndex] || null

    visible: ov.open === true && target !== null && client.connected
    screen: target
    color: Style.bg
    anchors.top: true
    anchors.bottom: true
    anchors.left: true
    anchors.right: true
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "nyx-overview"
    WlrLayershell.keyboardFocus: keyboardOff ? WlrKeyboardFocus.None : WlrKeyboardFocus.Exclusive

    property int serial: ov.serial || 0
    onSerialChanged: opened()
    onVisibleChanged: if (visible)
        opened()
    function opened() {
        search.clear();
        selectedAddress = flat.length ? flat[0].address : "";
        search.focusInput();
    }
    onFlatChanged: if (flat.length && !flat.some(i => i.address === selectedAddress))
        selectedAddress = flat[0].address

    function close() {
        client.request("OverviewClose", {}, null);
    }
    function activate(item) {
        if (item)
            client.request("OverviewActivate", { address: item.address, minimized: item.minimized }, null);
    }
    function act(action, item, to) {
        if (item)
            client.request("WindowAction", { action: action, address: item.address, to: to || "" }, null);
    }
    function moveBy(d) {
        if (flat.length)
            selectedAddress = flat[Math.max(0, Math.min(flat.length - 1, selectedIndex + d))].address;
    }
    // The nearest window above or below the selected one, by position on screen.
    function moveVertically(down) {
        const tiles = tileItems();
        const here = tiles.find(t => t.item.address === selectedAddress);
        if (!here)
            return;
        const hp = here.mapToItem(body, here.width / 2, here.height / 2);
        let best = null, bestD = 1e9;
        for (const t of tiles) {
            const p = t.mapToItem(body, t.width / 2, t.height / 2);
            if (down ? p.y <= hp.y + 4 : p.y >= hp.y - 4)
                continue;
            const d = Math.abs(p.y - hp.y) * 3 + Math.abs(p.x - hp.x);
            if (d < bestD) {
                bestD = d;
                best = t;
            }
        }
        if (best)
            selectedAddress = best.item.address;
    }
    function tileItems() {
        let out = [];
        const walk = it => {
            for (const c of it.children) {
                if (c.isOverviewTile)
                    out.push(c);
                else
                    walk(c);
            }
        };
        walk(body);
        return out;
    }

    FocusScope {
        anchors.fill: parent
        focus: true

        Keys.onPressed: e => {
            const ctrl = (e.modifiers & Qt.ControlModifier) !== 0;
            const item = w.selectedItem;
            const empty = search.text === "";
            if (e.key === Qt.Key_Escape) {
                if (empty)
                    w.close();
                else
                    search.clear();
            } else if (e.key === Qt.Key_Return || e.key === Qt.Key_Enter)
                w.activate(item);
            else if (e.key === Qt.Key_Tab)
                w.moveBy(1);
            else if (e.key === Qt.Key_Backtab)
                w.moveBy(-1);
            else if (e.key === Qt.Key_Left && (empty || ctrl))
                w.moveBy(-1);
            else if (e.key === Qt.Key_Right && (empty || ctrl))
                w.moveBy(1);
            else if (e.key === Qt.Key_Up)
                w.moveVertically(false);
            else if (e.key === Qt.Key_Down)
                w.moveVertically(true);
            else if (ctrl && e.key === Qt.Key_W)
                w.act("close", item);
            else if (ctrl && e.key === Qt.Key_G && item && item.grouped)
                w.act("ungroup", item);
            else if (ctrl && e.key === Qt.Key_T)
                w.act("float", item);
            else if (ctrl && e.key === Qt.Key_M && item) {
                const other = (w.ov.monitors || []).find(m => m !== item.monitor);
                if (other)
                    w.act("monitor", item, other);
            } else if (ctrl && e.key >= Qt.Key_1 && e.key <= Qt.Key_9)
                w.act("workspace", item, String(e.key - Qt.Key_0));
            else
                return;
            e.accepted = true;
        }

        // A click on the background (not on a window) closes.
        MouseArea {
            anchors.fill: parent
            onClicked: w.close()
        }

        Row {
            id: head
            anchors.horizontalCenter: parent.horizontalCenter
            y: 28
            spacing: 8
            Label {
                visible: !!(w.ov.profiles && w.ov.profiles.shown)
                anchors.verticalCenter: parent.verticalCenter
                text: w.ov.profiles ? w.ov.profiles.name || "" : ""
                size: 13
                font.weight: Font.DemiBold
                color: Style.dimText
            }
            IconButton {
                id: profileButton
                visible: !!(w.ov.profiles && w.ov.profiles.shown)
                anchors.verticalCenter: parent.verticalCenter
                icon: "view-paged-symbolic"
                tooltip: "Workspace profile: " + (w.ov.profiles ? w.ov.profiles.name || "" : "")
                onClicked: profileMenu.openAt(0, height + 4)
                Menu {
                    id: profileMenu
                    anchorItem: profileButton
                    entries: ((w.ov.profiles || {}).list || []).map(p => ({ id: p.id, label: p.name, checked: p.id === w.ov.profiles.active }))
                    onTriggered: id => w.client.request("Profile", { id: id }, null)
                }
            }
            SearchField {
                id: search
                width: 360
                placeholder: "Search windows"
                onTextChanged: w.client.request("OverviewQuery", { text: text }, null)
                onAccepted: w.activate(w.selectedItem)
                onEscaped: {
                    if (search.text === "")
                        w.close();
                    else
                        search.clear();
                }
            }
            IconButton {
                anchors.verticalCenter: parent.verticalCenter
                icon: "window-close-symbolic"
                tooltip: "Close · Esc"
                onClicked: w.close()
            }
        }

        Flickable {
            id: scroll
            anchors.top: head.bottom
            anchors.topMargin: 18
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            contentHeight: body.implicitHeight + 48
            clip: true
            boundsBehavior: Flickable.StopAtBounds

            Flow {
                id: body
                x: Math.max(48, (scroll.width - Math.min(implicitWidth, scroll.width - 96)) / 2)
                width: scroll.width - 96
                spacing: 18

                Label {
                    visible: w.groups.length === 0
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    topPadding: 120
                    text: search.text === "" ? "No windows open" : "No matching windows"
                    size: 14
                    color: Style.dimText
                }

                Repeater {
                    model: w.groups
                    delegate: Rectangle {
                        id: sec
                        required property var modelData
                        width: secCol.implicitWidth + 16
                        height: secCol.implicitHeight + 20
                        radius: 15
                        color: Style.card
                        MouseArea {
                            anchors.fill: parent
                        }
                        Column {
                            id: secCol
                            x: 8
                            y: 12
                            spacing: 6
                            Row {
                                x: 10
                                spacing: 8
                                Label {
                                    text: sec.modelData.title
                                    size: 14
                                    font.weight: Font.DemiBold
                                }
                                Label {
                                    visible: sec.modelData.meta !== ""
                                    anchors.baseline: parent.children[0].baseline
                                    text: sec.modelData.meta
                                    size: 12
                                    color: Style.dimText
                                }
                                HoverHandler {
                                    enabled: !sec.modelData.minimized && sec.modelData.workspace > 0
                                    cursorShape: Qt.PointingHandCursor
                                }
                                TapHandler {
                                    enabled: !sec.modelData.minimized && sec.modelData.workspace > 0
                                    onTapped: w.client.request("GoWorkspace", { workspace: sec.modelData.workspace }, null)
                                }
                            }
                            Grid {
                                columns: Math.max(1, sec.modelData.per_row)
                                spacing: 4
                                Repeater {
                                    model: sec.modelData.items
                                    delegate: WindowTile {
                                        id: tile
                                        required property var modelData
                                        readonly property bool isOverviewTile: true
                                        item: modelData
                                        thumbs: w.thumbs
                                        thumbnails: true
                                        showPlace: false
                                        hoverTint: true
                                        tile: w.ov.tile || ({ tile: 256, w: 232, h: 146, app: 112, window: 184 })
                                        selected: modelData.address === w.selectedAddress
                                        onClicked: w.activate(modelData)

                                        // Its actions, on the hovered or selected window.
                                        Rectangle {
                                            visible: tile.hovered || tile.selected
                                            anchors.right: parent.right
                                            anchors.rightMargin: (parent.width - tile.tile.w) / 2 + 6
                                            y: 6
                                            width: acts.implicitWidth + 4
                                            height: acts.implicitHeight + 4
                                            radius: 9
                                            color: Style.alpha(Style.popover, 0.94)
                                            Row {
                                                id: acts
                                                x: 2
                                                y: 2
                                                spacing: 0
                                                IconButton {
                                                    visible: tile.modelData.floating
                                                    width: 28
                                                    height: 28
                                                    icon: "focus-top-bar-symbolic"
                                                    tooltip: "It has its own titlebar: hide the extra one for this app"
                                                    onClicked: w.client.request("OwnTitlebar", { app: tile.modelData.app }, null)
                                                }
                                                IconButton {
                                                    visible: tile.modelData.grouped
                                                    width: 28
                                                    height: 28
                                                    icon: "edit-cut-symbolic"
                                                    tooltip: "Take out of the group · Ctrl+G"
                                                    onClicked: w.act("ungroup", tile.modelData)
                                                }
                                                IconButton {
                                                    width: 28
                                                    height: 28
                                                    icon: tile.modelData.floating ? "view-dual-symbolic" : "view-restore-symbolic"
                                                    tooltip: tile.modelData.floating ? "Tile · Ctrl+T" : "Float · Ctrl+T"
                                                    onClicked: w.act("float", tile.modelData)
                                                }
                                                IconButton {
                                                    id: moveButton
                                                    width: 28
                                                    height: 28
                                                    icon: "go-jump-symbolic"
                                                    tooltip: "Move to a workspace or monitor · Ctrl+1…9, Ctrl+M"
                                                    onClicked: moveMenu.openAt(0, height + 4)
                                                    Menu {
                                                        id: moveMenu
                                                        anchorItem: moveButton
                                                        entries: {
                                                            const it = tile.modelData;
                                                            let e = (w.ov.workspaces || []).filter(id => id !== it.workspace).map(id => ({ id: "ws:" + id, label: "Workspace " + id }));
                                                            e.push({ id: "ws:" + w.ov.free, label: "New workspace (" + w.ov.free + ")" });
                                                            const mons = (w.ov.monitors || []).filter(m => m !== it.monitor);
                                                            if (mons.length)
                                                                e.push({ separator: true });
                                                            for (const m of mons)
                                                                e.push({ id: "mon:" + m, label: "Monitor " + m });
                                                            return e;
                                                        }
                                                        onTriggered: id => {
                                                            if (id.startsWith("ws:"))
                                                                w.act("workspace", tile.modelData, id.substr(3));
                                                            else
                                                                w.act("monitor", tile.modelData, id.substr(4));
                                                        }
                                                    }
                                                }
                                                IconButton {
                                                    width: 28
                                                    height: 28
                                                    icon: "window-close-symbolic"
                                                    tooltip: "Close · Ctrl+W"
                                                    onClicked: w.act("close", tile.modelData)
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
