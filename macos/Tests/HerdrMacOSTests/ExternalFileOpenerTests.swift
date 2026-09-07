import Foundation
import Testing
@testable import HerdrMacOS

@Suite("External file opener")
struct ExternalFileOpenerTests {
    @Test("Default editor uses macOS' text-editor route for a checkout directory")
    @MainActor
    func defaultEditorUsesTextEditorRoute() async {
        let path = "/tmp/repository with spaces"
        var invocation: (URL, [String])?
        var failure: String?

        ExternalFileOpener.openInDefaultEditor(
            URL(fileURLWithPath: path),
            runner: { executable, arguments, completion in
                invocation = (executable, arguments)
                completion(0, "")
            },
            onFailure: { failure = $0 }
        )

        #expect(invocation?.0.path == "/usr/bin/open")
        #expect(invocation?.1 == ["-t", path])
        #expect(failure == nil)
    }

    @Test("Default editor failure reaches the user")
    @MainActor
    func defaultEditorFailureIsVisible() async throws {
        let path = "/tmp/repository"
        var failure: String?

        ExternalFileOpener.openInDefaultEditor(
            URL(fileURLWithPath: path),
            runner: { _, _, completion in
                completion(1, "editor refused the directory\n")
            },
            onFailure: { failure = $0 }
        )
        await Task.yield()

        #expect(failure == "The default editor could not open /tmp/repository: editor refused the directory")
    }
}
