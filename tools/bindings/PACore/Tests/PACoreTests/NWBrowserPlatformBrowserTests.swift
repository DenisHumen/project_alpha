// SPDX-License-Identifier: MPL-2.0
// NET-020: Bonjour through NWBrowser reaches the Rust core (LanDiscovery).
import Network
@testable import PACore
import XCTest

private final class Collector: DiscoveryListener, @unchecked Sendable {
    private let lock = NSLock()
    private var last: [LanService] = []

    func servicesChanged(services: [LanService]) {
        lock.lock()
        last = services
        lock.unlock()
    }

    var services: [LanService] {
        lock.lock()
        defer { lock.unlock() }
        return last
    }
}

final class NWBrowserPlatformBrowserTests: XCTestCase {
    func testBonjourServicesCoverTheBrowsedKinds() {
        let types = Set(bonjourServices())
        for type in ["_smb._tcp", "_webdav._tcp", "_webdavs._tcp", "_projectalpha._tcp"] {
            XCTAssertTrue(types.contains(type), type)
        }
        XCTAssertEqual(Set(discoverySourceKinds()), [.smb, .webDav, .webDavs, .server])
    }

    func testHostsAndTxtAsTheCoreWantsThem() {
        XCTAssertEqual(NWBrowserPlatformBrowser.address(.ipv4(IPv4Address("192.168.1.2")!)), "192.168.1.2")
        XCTAssertEqual(NWBrowserPlatformBrowser.address(.ipv6(IPv6Address("fd00::2")!)), "fd00::2")
        XCTAssertNil(NWBrowserPlatformBrowser.address(.name("nas.local", nil)))
        XCTAssertEqual(NWBrowserPlatformBrowser.string(.name("nas.local", nil)), "nas.local")
        let txt = NWBrowserPlatformBrowser.txtEntries(.bonjour(NWTXTRecord(["path": "/dav", "u": "x"])))
        XCTAssertEqual(txt, [TxtEntry(key: "path", value: Data("/dav".utf8)), TxtEntry(key: "u", value: Data("x".utf8))])
        XCTAssertEqual(NWBrowserPlatformBrowser.txtEntries(.none), [])
    }

    /// An NWListener announces a WebDAV share over the system Bonjour; the
    /// core finds it through NWBrowser, with the address from resolving.
    func testAnnouncedShareIsFoundWithItsAddress() throws {
        let name = "PACore test \(UUID().uuidString.prefix(8))"
        let listener = try NWListener(using: .tcp)
        listener.service = NWListener.Service(
            name: name,
            type: "_webdav._tcp",
            domain: nil,
            txtRecord: NWTXTRecord(["path": "/dav"])
        )
        let registered = expectation(description: "Bonjour registration")
        registered.assertForOverFulfill = false
        listener.serviceRegistrationUpdateHandler = { change in
            if case .add = change { registered.fulfill() }
        }
        listener.newConnectionHandler = { $0.cancel() }
        listener.start(queue: .global())
        defer { listener.cancel() }
        if XCTWaiter.wait(for: [registered], timeout: 15) != .completed {
            throw XCTSkip("no Bonjour responder on this host: the listener was never registered")
        }
        let port = try XCTUnwrap(listener.port?.rawValue)

        let discovery = LanDiscovery(browser: NWBrowserPlatformBrowser(), ownIds: [])
        defer { discovery.close() }
        let collector = Collector()
        try discovery.listen(listener: collector)
        try discovery.browse(kinds: [.webDav])

        let deadline = Date().addingTimeInterval(30)
        var found: LanService?
        while Date() < deadline {
            found = collector.services.first { $0.instance == name && $0.url != nil }
            if found != nil { break }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        let share = try XCTUnwrap(found, "not found: \(collector.services)")
        XCTAssertEqual(share.kind, .webDav)
        XCTAssertEqual(share.port, port)
        XCTAssertEqual(share.path, "/dav")
        XCTAssertTrue(share.url?.hasPrefix("http://") ?? false, share.url ?? "")
        XCTAssertTrue(share.url?.hasSuffix("/dav") ?? false, share.url ?? "")

        // Stopping a browse reports its instances lost (asynchronously).
        discovery.stop(kinds: [.webDav])
        let stopDeadline = Date().addingTimeInterval(5)
        while !discovery.services().isEmpty, Date() < stopDeadline {
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        XCTAssertTrue(discovery.services().isEmpty)
    }
}
