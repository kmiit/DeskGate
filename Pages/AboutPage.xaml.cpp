#include "pch.h"
#include "AboutPage.xaml.h"
#if __has_include("AboutPage.g.cpp")
#include "AboutPage.g.cpp"
#endif

using namespace winrt;
using namespace Microsoft::UI::Xaml;

// To learn more about WinUI, the WinUI project structure,
// and more about our project templates, see: http://aka.ms/winui-project-info.

namespace winrt::DeskGate::implementation
{
    int32_t AboutPage::MyProperty()
    {
        throw hresult_not_implemented();
    }

    void AboutPage::MyProperty(int32_t /* value */)
    {
        throw hresult_not_implemented();
    }

    void AboutPage::myButton_Click(IInspectable const&, RoutedEventArgs const&)
    {
        myButton().Content(box_value(L"Clicked"));
    }
}
