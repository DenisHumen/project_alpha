// SPDX-License-Identifier: MPL-2.0
// Metal surface for `MPVEngineConfiguration.VideoOutput.metal`.

#if canImport(QuartzCore) && (canImport(UIKit) || canImport(AppKit))
import QuartzCore

/// `CAMetalLayer` that ignores the 1×1 drawable size MoltenVK sets to
/// force-complete a presentation (avoids flicker; mpv PR #13651).
public final class MPVMetalLayer: CAMetalLayer {
    override public var drawableSize: CGSize {
        get { super.drawableSize }
        set {
            if Int(newValue.width) > 1, Int(newValue.height) > 1 {
                super.drawableSize = newValue
            }
        }
    }
}

#if canImport(UIKit)
import UIKit

/// A view backed by an `MPVMetalLayer`. Create the engine with
/// `.metal(view.metalLayer)`; the Flutter plugin (PLAY-APL-004) and the tvOS
/// app embed this view.
public final class MPVVideoView: UIView {
    override public class var layerClass: AnyClass { MPVMetalLayer.self }

    public var metalLayer: CAMetalLayer {
        // swiftlint:disable:next force_cast
        layer as! CAMetalLayer
    }

    override public init(frame: CGRect) {
        super.init(frame: frame)
        setUp()
    }

    public required init?(coder: NSCoder) {
        super.init(coder: coder)
        setUp()
    }

    private func setUp() {
        backgroundColor = .black
        metalLayer.framebufferOnly = true
        metalLayer.contentsScale = UIScreen.main.nativeScale
    }
}

#elseif canImport(AppKit)
import AppKit

/// A layer-hosting view with an `MPVMetalLayer` (macOS).
public final class MPVVideoView: NSView {
    public let metalLayer: CAMetalLayer = MPVMetalLayer()

    override public init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        setUp()
    }

    public required init?(coder: NSCoder) {
        super.init(coder: coder)
        setUp()
    }

    private func setUp() {
        metalLayer.framebufferOnly = true
        metalLayer.backgroundColor = NSColor.black.cgColor
        layer = metalLayer
        wantsLayer = true
    }

    override public func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        metalLayer.contentsScale = window?.backingScaleFactor ?? 1
    }
}
#endif
#endif
