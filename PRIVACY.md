# Talo privacy policy

Last updated: October 22, 2026

Talo is a desktop AI assistant. This policy describes how Talo handles information when you use the app, including its optional Google integration.

## Information Talo uses

Talo stores chats, settings, workspace information, and task history on your computer. Requests to an AI model are sent to the provider you configure, together with the conversation context and tool results needed to answer. That provider processes requests under its own privacy policy and your account settings.

Connecting Google is optional. With your permission, Talo can access Gmail messages and drafts, calendar events, and Drive file metadata and contents. It uses this access to answer your requests and carry out actions you authorize. Google content returned to a chat may be included in requests to your configured AI provider and saved in local chat history. If you enable other integrations, such as Telegram, content you choose to use through them also passes through those services.

Talo requests these Google permissions:

- `gmail.readonly`: find and read email messages.
- `gmail.compose`: create drafts and send email.
- `calendar.events`: read, create, update, and delete calendar events.
- `drive.readonly`: find and read Drive files.
- `drive.file`: create and update files accessible to Talo under this permission.

Talo asks for approval before its Google tools send email, create drafts, change calendar events, or write Drive files. Read access can be used by chats and tasks that you authorize.

## Storage and sharing

Google sign-in takes place in your system browser. Talo does not receive your Google password. On Windows, Talo protects stored Google access and refresh tokens with Windows user-bound encryption. These tokens are used by the desktop host to contact Google and are not supplied to the AI model.

Google data is shared with Google to perform API requests and with your configured AI provider when needed for the assistant features you use. Talo does not sell Google user data, use it for advertising, or train its own AI models on it. Talo's use and transfer of information received from Google APIs will adhere to the Google API Services User Data Policy, including its Limited Use requirements: https://developers.google.com/terms/api-services-user-data-policy.

## Your choices and deletion

You can disconnect Google in Talo's settings. This removes the local sign-in and attempts to revoke its Google authorization. You can also revoke access at https://myaccount.google.com/permissions. Revoking or disconnecting does not remove content already saved in chats, delete files or messages in Google, or remove information already processed by an external provider.

Local chat and task information remains on your computer until you delete it. You can remove Talo's local app data to remove its stored information; uninstalling alone may leave app data behind. External providers and Google apply their own retention and deletion policies to information they process.

## Contact and changes

For privacy questions, contact the maintainer through https://github.com/Vortextbloons/talo/issues. Do not include private messages, credentials, or tokens in public issues. Updates to this policy will be published in this repository with a revised date.
