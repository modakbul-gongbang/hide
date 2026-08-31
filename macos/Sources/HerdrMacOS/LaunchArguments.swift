/// Reads `--flag value` pairs out of a launch argument vector.
///
/// The shell and its verification surfaces are configured entirely through
/// launch flags, so every surface that takes one reads it the same way.
enum LaunchArguments {
    static func value(_ flag: String, in arguments: [String]) -> String? {
        guard let index = arguments.firstIndex(of: flag), arguments.indices.contains(index + 1) else {
            return nil
        }
        return arguments[index + 1]
    }
}
