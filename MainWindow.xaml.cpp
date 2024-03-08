#include "pch.h"
#include "MainWindow.xaml.h"
#if __has_include("MainWindow.g.cpp")
#include "MainWindow.g.cpp"
#endif
#include <winrt/Windows.UI.Xaml.Interop.h>

using namespace winrt;
using namespace Microsoft::UI::Xaml;

// To learn more about WinUI, the WinUI project structure,
// and more about our project templates, see: http://aka.ms/winui-project-info.

namespace winrt::DeskGate::implementation
{
	MainWindow::MainWindow()
	{
		InitializeComponent();
		ContentFrame().Navigate(xaml_typename<DeskGate::BasicPage>());
	}

	void MainWindow::NavigationView_SelectionChanged(winrt::Microsoft::UI::Xaml::Controls::NavigationView const& sender,
		winrt::Microsoft::UI::Xaml::Controls::NavigationViewSelectionChangedEventArgs const& args)
	{
		auto selectedItem = sender.SelectedItem().as<Controls::NavigationViewItem>();
		auto tag = selectedItem.Tag().as<hstring>();
		if (tag == L"basic")
		{
			ContentFrame().Navigate(xaml_typename<DeskGate::BasicPage>(), *this);
		}
		else if (tag == L"advanced")
		{
			ContentFrame().Navigate(xaml_typename<DeskGate::AdvancedPage>(), *this);
		}
		else if (tag == L"about")
		{
			ContentFrame().Navigate(xaml_typename<DeskGate::AboutPage>(), *this);
		}

	}
}