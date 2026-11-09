import QtQuick
import Nyx.Ui

// A window: its thumbnail (the app's icon until one arrives, or always without
// thumbnails), title and place. Also the app switcher's tile (`apps`): the icon and how
// many windows.
Rectangle {
    id: t

    required property var item
    property var thumbs: ({})
    property bool selected: false
    property bool thumbnails: true
    property bool apps: false
    property bool showPlace: true
    property bool hoverTint: false
    property var tile: ({ tile: 256, w: 232, h: 146, app: 112, window: 184 })
    readonly property string shot: item && item.stable_id && thumbs[item.stable_id] ? "file://" + thumbs[item.stable_id] : ""
    readonly property bool hovered: hover.hovered
    default property alias overlay: frame.data

    signal clicked
    signal pointed

    width: (apps ? tile.app : thumbnails ? tile.tile : tile.window) - 4
    height: content.implicitHeight + 16
    radius: 11
    color: selected ? Style.selected : hoverTint && hovered ? Style.rowHover : "transparent"
    border.width: selected ? 2 : 0
    border.color: Style.alpha(Style.accent, 0.85)
    Behavior on color {
        ColorAnimation {
            duration: Style.reducedMotion ? 0 : 90
        }
    }

    HoverHandler {
        id: hover
        onPointChanged: t.pointed()
    }
    TapHandler {
        onTapped: t.clicked()
    }

    Column {
        id: content
        x: 8
        y: 8
        width: t.width - 16
        spacing: 6

        Item {
            id: frame
            visible: !t.apps
            width: parent.width
            height: t.thumbnails ? t.tile.h : 66
            Rectangle {
                visible: t.thumbnails
                anchors.horizontalCenter: parent.horizontalCenter
                width: t.tile.w
                height: t.tile.h
                radius: 7
                color: Style.alpha(Style.text, 0.05)
                clip: true
                Image {
                    id: pic
                    anchors.fill: parent
                    source: t.shot
                    fillMode: Image.PreserveAspectFit
                    asynchronous: true
                    cache: false
                    smooth: true
                }
            }
            AppIcon {
                anchors.centerIn: parent
                anchors.verticalCenterOffset: t.thumbnails ? 0 : 5
                visible: !t.thumbnails || pic.status !== Image.Ready
                names: t.item ? t.item.icons : []
                size: 56
            }
            AppIcon {
                visible: t.thumbnails && pic.status === Image.Ready
                x: (parent.width - t.tile.w) / 2 - 2
                y: parent.height - size + 2
                names: t.item ? t.item.icons : []
                size: 28
            }
            Rectangle {
                visible: !!(t.item && t.item.tabs)
                x: (parent.width - t.tile.w) / 2 + 6
                y: 6
                width: tabsLabel.implicitWidth + 14
                height: tabsLabel.implicitHeight + 2
                radius: 7
                color: Style.alpha(Style.popover, 0.92)
                Label {
                    id: tabsLabel
                    anchors.centerIn: parent
                    text: t.item ? t.item.tabs || "" : ""
                    size: 11
                    font.weight: Font.DemiBold
                }
            }
        }

        Item {
            visible: t.apps
            width: parent.width
            height: t.tile.app - 16
            AppIcon {
                anchors.centerIn: parent
                names: t.item ? t.item.icons : []
                size: 72
            }
            Rectangle {
                visible: t.item && t.item.windows > 1
                anchors.right: parent.right
                anchors.top: parent.top
                width: Math.max(18, count.implicitWidth + 10)
                height: 18
                radius: 9
                color: Style.raised
                Label {
                    id: count
                    anchors.centerIn: parent
                    text: t.item ? String(t.item.windows) : ""
                    size: 11
                    font.weight: Font.DemiBold
                }
            }
        }

        Label {
            visible: !t.apps
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            text: t.item ? t.item.title : ""
            size: 13
            font.weight: Font.Medium
            elide: Text.ElideRight
        }
        Label {
            visible: !t.apps && t.showPlace
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            text: t.item ? t.item.place : ""
            size: 11.5
            color: t.item && t.item.urgent ? Style.accent : Style.dimText
            elide: Text.ElideRight
        }
    }
}
