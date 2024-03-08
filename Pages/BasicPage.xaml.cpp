#include "pch.h"
#include "BasicPage.xaml.h"
#if __has_include("BasicPage.g.cpp")
#include "BasicPage.g.cpp"
#include <stddef.h>
#endif

using namespace winrt;
using namespace Microsoft::UI::Xaml;
using namespace Microsoft::UI::Xaml::Controls;
using namespace Microsoft::UI::Xaml::Navigation;

namespace winrt::DeskGate::implementation
{
	BasicPage::BasicPage()
	{
		InitializeComponent();
	}

	void BasicPage::ChangeConfig(winrt::Windows::Foundation::IInspectable const&, winrt::Microsoft::UI::Xaml::RoutedEventArgs const& e)
	{
		//auto config = Windows::Storage::ApplicationData::Current().LocalSettings().Values().Insert(L"config", L"");)
	}

}