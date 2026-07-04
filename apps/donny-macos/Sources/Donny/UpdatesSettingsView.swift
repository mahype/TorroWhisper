import SwiftUI

struct UpdatesSettingsView: View {
    @ObservedObject var updaterController: UpdaterController

    var body: some View {
        Section {
            Toggle(isOn: $updaterController.automaticallyChecksForUpdates) {
                Text("Automatically check for updates", bundle: .module)
            }
            .disabled(!updaterController.isAvailable)

            Button {
                updaterController.checkForUpdates()
            } label: {
                Text("Check for updates now", bundle: .module)
            }
            .disabled(!updaterController.isAvailable)

            if updaterController.isAvailable {
                HStack(spacing: 4) {
                    Text("Last checked:", bundle: .module)
                    if let date = updaterController.lastUpdateCheckDate {
                        Text(date, format: .dateTime.day().month().year().hour().minute())
                    } else {
                        Text("Never", bundle: .module)
                    }
                }
                .font(.callout)
                .foregroundStyle(.secondary)
            }
        } header: {
            Text("Automatic updates", bundle: .module)
        }

        Section {
            if updaterController.isAvailable {
                Text("Donny checks for new versions at launch and then every 24 hours. Updates download in the background and install the next time you restart.", bundle: .module)
                    .font(.callout)
                    .foregroundStyle(.secondary)
            } else {
                Text("Updates are only available in the installed .app (dev build).", bundle: .module)
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
        }
    }
}
